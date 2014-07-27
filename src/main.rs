pub use color::Color4;
use bounding_box::*;
use cgmath::aabb::Aabb2;
use cgmath::angle;
use cgmath::array::Array2;
use cgmath::matrix::{Matrix, Matrix3, Matrix4};
use cgmath::num::{BaseFloat};
use cgmath::point::{Point2, Point3};
use cgmath::vector::{Vector, Vector3};
use cgmath::projection;
use cstr_cache::CStringCache;
use fontloader;
use piston;
use piston::*;
use gl;
use gl::types::*;
use glbuffer::GLBuffer;
use sdl2_game_window::GameWindowSDL2;
use sdl2::mouse;
use stopwatch;
use std::collections::HashMap;
use std::mem;
use std::iter::range_inclusive;
use std::ptr;
use std::str;
use std::num;
use vertex;
use vertex::{ColoredVertex,TextureVertex};

// TODO(cgaebel): How the hell do I get this to be exported from `mod stopwatch`?
macro_rules! time(
  ($timers:expr, $name:expr, $f:expr) => (
    unsafe { ($timers as *const stopwatch::TimerSet).to_option() }.unwrap().time($name, $f)
  );
)

static WINDOW_WIDTH: u32 = 800;
static WINDOW_HEIGHT: u32 = 600;

// much bigger than 200000 starts segfaulting.
static MAX_WORLD_SIZE: uint = 100000;

static MAX_JUMP_FUEL: uint = 4;

// how many blocks to load during every update step
static LOAD_SPEED:uint = 1 << 12;
static SKY_COLOR: Color4<GLfloat>  = Color4 {r: 0.2, g: 0.5, b: 0.7, a: 1.0 };

#[deriving(Clone)]
#[allow(missing_doc)]
pub enum BlockType {
  Grass,
  Dirt,
  Stone,
}

impl BlockType {
  fn to_color(&self) -> Color4<GLfloat> {
    match *self {
      Grass => Color4::of_rgba(0.0, 0.5,  0.0, 1.0),
      Dirt  => Color4::of_rgba(0.2, 0.15, 0.1, 1.0),
      Stone => Color4::of_rgba(0.5, 0.5,  0.5, 1.0),
    }
  }
}

#[deriving(Clone)]
/// A voxel-ish block in the game world.
pub struct Block {
  // bounds of the Block
  block_type: BlockType,
  id: u32,
}

impl Block {
  #[inline]
  fn to_triangles(block: &Block, bounds: &BoundingBox) -> [ColoredVertex, ..VERTICES_PER_TRIANGLE * TRIANGLES_PER_BOX] {
    let colors = [block.block_type.to_color(), ..6];
    bounds.to_triangles(colors)
  }

  // Construct outlines for this Block, to sharpen the edges.
  fn to_outlines(bounds: &BoundingBox) -> [ColoredVertex, ..VERTICES_PER_LINE * LINES_PER_BOX] {
    // distance from the block to construct the bounding outlines.
    let d = 0.002;
    let (x1, y1, z1) = (bounds.low_corner.x - d, bounds.low_corner.y - d, bounds.low_corner.z - d);
    let (x2, y2, z2) = (bounds.high_corner.x + d, bounds.high_corner.y + d, bounds.high_corner.z + d);
    let c = Color4::of_rgba(0.0, 0.0, 0.0, 1.0);

    let vtx = |x: GLfloat, y: GLfloat, z: GLfloat| -> ColoredVertex {
      ColoredVertex {
        position: Point3 { x: x, y: y, z: z },
        color: c
      }
    };

    [
      vtx(x1, y1, z1), vtx(x2, y1, z1),
      vtx(x1, y2, z1), vtx(x2, y2, z1),
      vtx(x1, y1, z2), vtx(x2, y1, z2),
      vtx(x1, y2, z2), vtx(x2, y2, z2),

      vtx(x1, y1, z1), vtx(x1, y2, z1),
      vtx(x2, y1, z1), vtx(x2, y2, z1),
      vtx(x1, y1, z2), vtx(x1, y2, z2),
      vtx(x2, y1, z2), vtx(x2, y2, z2),

      vtx(x1, y1, z1), vtx(x1, y1, z2),
      vtx(x2, y1, z1), vtx(x2, y1, z2),
      vtx(x1, y2, z1), vtx(x1, y2, z2),
      vtx(x2, y2, z1), vtx(x2, y2, z2),
    ]
  }
}

pub struct Player {
  // speed; units are world coordinates
  speed: Vector3<GLfloat>,
  // acceleration; x/z units are relative to player facing
  accel: Vector3<GLfloat>,
  // this is depleted as we jump and replenished as we stand.
  jump_fuel: uint,
  // are we currently trying to jump? (e.g. holding the key).
  is_jumping: bool,
  id: u32,
}

#[inline]
/// `expect` an Option with a message assuming it is the result of an entity
/// id lookup.
fn expect_id<T>(v: Option<T>) -> T {
  v.expect("expected entity id not found")
}

/// The whole application. Wrapped up in a nice frameworky struct for piston.
pub struct App {
  physics: HashMap<u32, BoundingBox>,
  blocks: HashMap<u32, Block>,
  player: Player,
  // id of the next block to load
  next_load_id: u32,
  // next block id to assign
  next_block_id: u32,
  // map index in GLBuffers to entity id
  index_to_id: Vec<u32>,
  // mapping of entity id to the block's index in GLBuffers
  id_to_index: HashMap<u32, uint>,
  // OpenGL buffers
  world_triangles: GLBuffer<ColoredVertex>,
  outlines: GLBuffer<ColoredVertex>,
  hud_triangles: GLBuffer<ColoredVertex>,
  texture_triangles: GLBuffer<TextureVertex>,
  textures: Vec<GLuint>,
  // OpenGL-friendly equivalent of physics for selection/picking.
  selection_triangles: GLBuffer<ColoredVertex>,
  // OpenGL projection matrix components
  hud_matrix: Matrix4<GLfloat>,
  fov_matrix: Matrix4<GLfloat>,
  translation_matrix: Matrix4<GLfloat>,
  rotation_matrix: Matrix4<GLfloat>,
  lateral_rotation: angle::Rad<GLfloat>,
  vertical_rotation: angle::Rad<GLfloat>,
  // OpenGL shader "program" id.
  shader_program: u32,
  texture_shader: u32,

  // which mouse buttons are currently pressed
  mouse_buttons_pressed: Vec<piston::mouse::Button>,

  font: fontloader::FontLoader,
  scache: CStringCache,

  timers: stopwatch::TimerSet,
}

/// Create a 3D translation matrix.
pub fn translate(t: Vector3<GLfloat>) -> Matrix4<GLfloat> {
  Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    t.x, t.y, t.z, 1.0,
  )
}

/// Create a 3D perspective initialization matrix.
pub fn perspective(fovy: GLfloat, aspect: GLfloat, near: GLfloat, far: GLfloat) -> Matrix4<GLfloat> {
  Matrix4::new(
    fovy / aspect, 0.0, 0.0,                              0.0,
    0.0,          fovy, 0.0,                              0.0,
    0.0,           0.0, (near + far) / (near - far),     -1.0,
    0.0,           0.0, 2.0 * near * far / (near - far),  0.0,
  )
}

#[inline]
/// Create a XY symmetric ortho matrix.
pub fn sortho(dx: GLfloat, dy: GLfloat, near: GLfloat, far: GLfloat) -> Matrix4<GLfloat> {
  projection::ortho(-dx, dx, -dy, dy, near, far)
}

/// Create a matrix from a rotation around an arbitrary axis.
pub fn from_axis_angle<S: BaseFloat>(axis: Vector3<S>, angle: angle::Rad<S>) -> Matrix4<S> {
    let (s, c) = angle::sin_cos(angle);
    let _1subc = num::one::<S>() - c;

    Matrix4::new(
        _1subc * axis.x * axis.x + c,
        _1subc * axis.x * axis.y + s * axis.z,
        _1subc * axis.x * axis.z - s * axis.y,
        num::zero(),

        _1subc * axis.x * axis.y - s * axis.z,
        _1subc * axis.y * axis.y + c,
        _1subc * axis.y * axis.z + s * axis.x,
        num::zero(),

        _1subc * axis.x * axis.z + s * axis.y,
        _1subc * axis.y * axis.z - s * axis.x,
        _1subc * axis.z * axis.z + c,
        num::zero(),

        num::zero(),
        num::zero(),
        num::zero(),
        num::one(),
    )
}

#[inline]
pub fn swap_remove_first<T: PartialEq + Copy>(v: &mut Vec<T>, t: T) {
  match v.iter().position(|x| { *x == t }) {
    None => { },
    Some(i) => { v.swap_remove(i); },
  }
}

impl Game<GameWindowSDL2> for App {
  fn key_press(&mut self, _: &mut GameWindowSDL2, args: &KeyPressArgs) {
    time!(&self.timers, "event.key_press", || unsafe {
      match args.key {
        piston::keyboard::A => {
          self.walk(-Vector3::unit_x());
        },
        piston::keyboard::D => {
          self.walk(Vector3::unit_x());
        },
        piston::keyboard::LShift => {
          self.walk(-Vector3::unit_y());
        },
        piston::keyboard::Space => {
          if !self.player.is_jumping {
            self.player.is_jumping = true;
            // this 0.3 is duplicated in a few places
            self.player.accel.y = self.player.accel.y + 0.3;
          }
        },
        piston::keyboard::W => {
          self.walk(-Vector3::unit_z());
        },
        piston::keyboard::S => {
          self.walk(Vector3::unit_z());
        },
        piston::keyboard::Left =>
          self.rotate_lateral(angle::rad(3.14 / 12.0 as GLfloat)),
        piston::keyboard::Right =>
          self.rotate_lateral(angle::rad(-3.14 / 12.0 as GLfloat)),
        piston::keyboard::Up =>
          self.rotate_vertical(angle::rad(3.14/12.0 as GLfloat)),
        piston::keyboard::Down =>
          self.rotate_vertical(angle::rad(-3.14/12.0 as GLfloat)),
        _ => {},
      }
    })
  }

  fn key_release(&mut self, _: &mut GameWindowSDL2, args: &KeyReleaseArgs) {
    time!(&self.timers, "event.key_release", || {
      match args.key {
        // accelerations are negated from those in key_press.
        piston::keyboard::A => {
          self.walk(Vector3::unit_x());
        },
        piston::keyboard::D => {
          self.walk(-Vector3::unit_x());
        },
        piston::keyboard::LShift => {
          self.walk(Vector3::unit_y());
        },
        piston::keyboard::Space => {
          if self.player.is_jumping {
            self.player.is_jumping = false;
            // this 0.3 is duplicated in a few places
            self.player.accel.y = self.player.accel.y - 0.3;
          }
        },
        piston::keyboard::W => {
          self.walk(Vector3::unit_z());
        },
        piston::keyboard::S => {
          self.walk(-Vector3::unit_z());
        },
        _ => { }
      }
    })
  }

  fn mouse_move(&mut self, w: &mut GameWindowSDL2, args: &MouseMoveArgs) {
    time!(&self.timers, "event.mouse_move", || unsafe {
      let (cx, cy) = (WINDOW_WIDTH as f32 / 2.0, WINDOW_HEIGHT as f32 / 2.0);
      // args.y = h - args.y;
      // dy = args.y - cy;
      //  => dy = cy - args.y;
      let (dx, dy) = (args.x as f32 - cx, cy - args.y as f32);
      let (rx, ry) = (dx * -3.14 / 1024.0, dy * 3.14 / 1024.0);
      self.rotate_lateral(angle::rad(rx));
      self.rotate_vertical(angle::rad(ry));

      mouse::warp_mouse_in_window(&w.render_window.window, WINDOW_WIDTH as i32 / 2, WINDOW_HEIGHT as i32 / 2);
    })
  }

  fn mouse_press(&mut self, _: &mut GameWindowSDL2, args: &MousePressArgs) {
    time!(&self.timers, "event.mouse_press", || {
      self.mouse_buttons_pressed.push(args.button);
    })
  }

  fn mouse_release(&mut self, _: &mut GameWindowSDL2, args: &MouseReleaseArgs) {
    swap_remove_first(&mut self.mouse_buttons_pressed, args.button)
  }

  fn load(&mut self, _: &mut GameWindowSDL2) {
    time!(&self.timers, "load", || {
      mouse::show_cursor(false);

      gl::FrontFace(gl::CCW);
      gl::CullFace(gl::BACK);
      gl::Enable(gl::CULL_FACE);

      gl::Enable(gl::BLEND);
      gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

      gl::Enable(gl::LINE_SMOOTH);
      gl::LineWidth(2.5);

      gl::Enable(gl::DEPTH_TEST);
      gl::DepthFunc(gl::LESS);
      gl::ClearDepth(100.0);
      gl::ClearColor(SKY_COLOR.r, SKY_COLOR.g, SKY_COLOR.b, SKY_COLOR.a);

      unsafe {
        self.set_up_shaders();

        // initialize the projection matrix
        self.fov_matrix = perspective(3.14/3.0, 4.0/3.0, 0.1, 100.0);
        self.translate(Vector3::new(0.0, 4.0, 10.0));
        self.update_projection();
      }

      let timers = &self.timers;

      unsafe {
        self.selection_triangles = GLBuffer::new(
          self.shader_program,
          [ vertex::AttribData { name: "position", size: 3 },
            vertex::AttribData { name: "in_color", size: 4 },
          ],
          MAX_WORLD_SIZE * TRIANGLE_VERTICES_PER_BOX,
        );

        self.world_triangles = GLBuffer::new(
          self.shader_program,
          [ vertex::AttribData { name: "position", size: 3 },
            vertex::AttribData { name: "in_color", size: 4 },
          ],
          MAX_WORLD_SIZE * TRIANGLE_VERTICES_PER_BOX,
        );

        self.outlines = GLBuffer::new(
          self.shader_program,
          [ vertex::AttribData { name: "position", size: 3 },
            vertex::AttribData { name: "in_color", size: 4 },
          ],
          MAX_WORLD_SIZE * LINE_VERTICES_PER_BOX,
        );

        self.hud_triangles = GLBuffer::new(
          self.shader_program,
          [ vertex::AttribData { name: "position", size: 3 },
            vertex::AttribData { name: "in_color", size: 4 },
          ],
          16 * VERTICES_PER_TRIANGLE,
        );

        self.texture_triangles = GLBuffer::new(
          self.texture_shader,
          [ vertex::AttribData { name: "position", size: 2 },
            vertex::AttribData { name: "texture_position", size: 2 },
          ],
          8 * VERTICES_PER_TRIANGLE,
        );


        self.make_textures();
        self.make_hud();
      }

      timers.time("load.construct", || unsafe {
        // low dirt block
        for i in range_inclusive(-2i, 2) {
          for j in range_inclusive(-2i, 2) {
            let (i, j) = (i as GLfloat / 2.0, j as GLfloat / 2.0);
            let (x1, y1, z1) = (6.0 + i, 6.0, 0.0 + j);
            let (x2, y2, z2) = (6.5 + i, 6.5, 0.5 + j);
            self.place_block(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Dirt, false);
          }
        }
        // high dirt block
        for i in range_inclusive(-2i, 2) {
          for j in range_inclusive(-2i, 2) {
            let (i, j) = (i as GLfloat / 2.0, j as GLfloat / 2.0);
            let (x1, y1, z1) = (0.0 + i, 12.0, 5.0 + j);
            let (x2, y2, z2) = (0.5 + i, 12.5, 5.5 + j);
            self.place_block(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Dirt, false);
          }
        }
        // ground
        for i in range_inclusive(-64i, 64) {
          for j in range_inclusive(-64i, 64) {
            let (i, j) = (i as GLfloat / 2.0, j as GLfloat / 2.0);
            let (x1, y1, z1) = (i, 0.0, j);
            let (x2, y2, z2) = (i + 0.5, 0.5, j + 0.5);
            self.place_block(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Grass, false);
          }
        }
        // front wall
        for i in range_inclusive(-64i, 64) {
          for j in range_inclusive(0i, 64) {
            let (i, j) = (i as GLfloat / 2.0, j as GLfloat / 2.0);
            let (x1, y1, z1) = (i, 0.5 + j, -32.0);
            let (x2, y2, z2) = (i + 0.5, 1.0 + j, -32.0 + 0.5);
            self.place_block(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone, false);
          }
        }
        // back wall
        for i in range_inclusive(-64i, 64) {
          for j in range_inclusive(0i, 64) {
            let (i, j) = (i as GLfloat / 2.0, j as GLfloat / 2.0);
            let (x1, y1, z1) = (i - 0.5, 0.5 + j, 32.0);
            let (x2, y2, z2) = (i + 0.5, 1.0 + j, 32.0 + 0.5);
            self.place_block(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone, false);
          }
        }
        // left wall
        for i in range_inclusive(-64i, 64) {
          for j in range_inclusive(0i, 64) {
            let (i, j) = (i as GLfloat / 2.0, j as GLfloat / 2.0);
            let (x1, y1, z1) = (-32.0, 0.5 + j, i - 0.5);
            let (x2, y2, z2) = (-32.0 + 0.5, 1.0 + j, i + 0.5);
            self.place_block(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone, false);
          }
        }
        // right wall
        for i in range_inclusive(-64i, 64) {
          for j in range_inclusive(0i, 64) {
            let (i, j) = (i as GLfloat / 2.0, j as GLfloat / 2.0);
            let (x1, y1, z1) = (32.0, 0.5 + j, i);
            let (x2, y2, z2) = (32.0 + 0.5, 1.0 + j, i + 0.5);
            self.place_block(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone, false);
          }
        }
      });
    })

    println!("load() finished with {} blocks", self.blocks.len());
  }

  fn update(&mut self, _: &mut GameWindowSDL2, _: &UpdateArgs) {
    time!(&self.timers, "update", || unsafe {
      if self.next_load_id < self.next_block_id {
        time!(&self.timers, "update.load", || unsafe {
          let mut i = 0;
          let mut triangles = Vec::new();
          let mut outlines = Vec::new();
          let mut selections = Vec::new();
          while i < LOAD_SPEED && self.next_load_id < self.next_block_id {
            self.blocks.find(&self.next_load_id).map(|block| {
              let bounds = self.physics.find(&self.next_load_id).expect("phyiscs prematurely deleted");
              triangles.push_all(Block::to_triangles(block, bounds));
              outlines.push_all(Block::to_outlines(bounds));
              let selection_id = block.id * 6;
              let selection_colors =
                    [ id_color(selection_id + 0),
                      id_color(selection_id + 1),
                      id_color(selection_id + 2),
                      id_color(selection_id + 3),
                      id_color(selection_id + 4),
                      id_color(selection_id + 5),
                    ];
              selections.push_all(bounds.to_triangles(selection_colors));
            });

            self.next_load_id += 1;
            i += 1;
          }

          if triangles.len() > 0 {
            self.world_triangles.push(triangles.slice(0, triangles.len()));
            self.outlines.push(outlines.slice(0, outlines.len()));
            self.selection_triangles.push(selections.slice(0, selections.len()));
          }
        })
      }

      time!(&self.timers, "update.player", || unsafe {
        if self.player.is_jumping {
          if self.player.jump_fuel > 0 {
            self.player.jump_fuel -= 1;
          } else {
            // this code is duplicated in a few places
            self.player.is_jumping = false;
            self.player.accel.y = self.player.accel.y - 0.3;
          }
        }

        let dP = self.player.speed;
        if dP.x != 0.0 {
          self.translate(Vector3::new(dP.x, 0.0, 0.0));
        }
        if dP.y != 0.0 {
          self.translate(Vector3::new(0.0, dP.y, 0.0));
        }
        if dP.z != 0.0 {
          self.translate(Vector3::new(0.0, 0.0, dP.z));
        }

        let dV = Matrix3::from_axis_angle(&Vector3::unit_y(), self.lateral_rotation).mul_v(&self.player.accel);
        self.player.speed = self.player.speed + dV;
        // friction
        self.player.speed = self.player.speed * Vector3::new(0.7, 0.99, 0.7);
      });

      // Block deletion
      if self.is_mouse_pressed(piston::mouse::Left) {
        time!(&self.timers, "update.delete_block", || unsafe {
          self.block_at_window_center().map(|(id, _)| { self.remove_block(id) });
        })
      }
      if self.is_mouse_pressed(piston::mouse::Right) {
        time!(&self.timers, "update.place_block", || unsafe {
          self.block_at_window_center().map(|(block_id, face)| {
            let bounds = expect_id(self.physics.find(&block_id));
            let direction =
                  [ -Vector3::unit_z(),
                    -Vector3::unit_x(),
                      Vector3::unit_y(),
                      Vector3::unit_z(),
                      Vector3::unit_x(),
                    -Vector3::unit_y(),
                  ][face].mul_s(0.5);
            // TODO: think about how this should work when placing size A blocks
            // against size B blocks.
            self.place_block(
              bounds.low_corner + direction,
              bounds.high_corner + direction,
              Dirt,
              true
            );
          });
        })
      }
    })
  }

  fn render(&mut self, _: &mut GameWindowSDL2, _: &RenderArgs) {
    time!(&self.timers, "render", || unsafe {
      // set the sky color
      gl::ClearColor(SKY_COLOR.r, SKY_COLOR.g, SKY_COLOR.b, SKY_COLOR.a);

      // draw the world
      gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
      self.world_triangles.draw(gl::TRIANGLES);
      self.outlines.draw(gl::LINES);

      // draw the hud
      self.set_projection(&self.hud_matrix);
      self.hud_triangles.draw(gl::TRIANGLES);
      self.update_projection();

      // draw textures
      gl::UseProgram(self.texture_shader);
      let mut i = 0u;
      for tex in self.textures.iter() {
        gl::BindTexture(gl::TEXTURE_2D, *tex);
        self.texture_triangles.draw_slice(gl::TRIANGLES, i * 6, 6);
        i += 1;
      }
      gl::UseProgram(self.shader_program);
    })
  }
}

#[inline]
fn mask(mask: u32, i: u32) -> u32 {
  (i & mask) >> (mask as uint).trailing_zeros()
}

// map ids to unique colors
fn id_color(id: u32) -> Color4<GLfloat> {
  assert!(id < 0xFF000000, "too many items for selection buffer");
  let ret = Color4::of_rgba(
    (mask(0x00FF0000, id) as GLfloat / 255.0),
    (mask(0x0000FF00, id) as GLfloat / 255.0),
    (mask(0x000000FF, id) as GLfloat / 255.0),
    1.0,
  );
  assert!(ret.r >= 0.0);
  assert!(ret.r <= 1.0 as f32);
  assert!(ret.g >= 0.0 as f32);
  assert!(ret.g <= 1.0 as f32);
  assert!(ret.b >= 0.0 as f32);
  assert!(ret.b <= 1.0 as f32);
  ret
}

impl App {
  /// Initializes an empty app.
  pub unsafe fn new() -> App {
    App {
      physics: {
        let mut h = HashMap::new();
        h.insert(1, BoundingBox {
            low_corner: Vector3::new(-1.0, -2.0, -1.0),
            high_corner: Vector3::zero(),
        });
        h
      },
      blocks: HashMap::new(),
      player: Player {
        speed: Vector3::zero(),
        accel: Vector3::new(0.0, -0.1, 0.0),
        jump_fuel: 0,
        is_jumping: false,
        id: 1,
      },
      next_load_id: 2,
      // Start assigning block_ids at 1.
      // block_id 0 corresponds to no block.
      next_block_id: 2,
      index_to_id: Vec::new(),
      id_to_index: HashMap::new(),
      world_triangles: GLBuffer::null(),
      outlines: GLBuffer::null(),
      hud_triangles: GLBuffer::null(),
      selection_triangles: GLBuffer::null(),
      texture_triangles: GLBuffer::null(),
      textures: Vec::new(),
      hud_matrix: translate(Vector3::new(0.0, 0.0, -1.0)) * sortho(WINDOW_WIDTH as f32 / WINDOW_HEIGHT as f32, 1.0, -1.0, 1.0),
      fov_matrix: Matrix4::identity(),
      translation_matrix: Matrix4::identity(),
      rotation_matrix: Matrix4::identity(),
      lateral_rotation: angle::rad(0.0),
      vertical_rotation: angle::rad(0.0),
      shader_program: -1 as u32,
      texture_shader: -1 as u32,
      mouse_buttons_pressed: Vec::new(),
      font: fontloader::FontLoader::new(),
      scache: CStringCache::new(),
      timers: stopwatch::TimerSet::new(),
    }
  }

  /// Build all of our program's shaders.
  pub unsafe fn set_up_shaders(&mut self) {
    let ivs = compile_shader(ID_VS_SRC, gl::VERTEX_SHADER);
    let txs = compile_shader(TX_SRC, gl::FRAGMENT_SHADER);
    self.texture_shader = link_program(ivs, txs);
    gl::UseProgram(self.texture_shader);

    let vs = compile_shader(VS_SRC, gl::VERTEX_SHADER);
    let fs = compile_shader(FS_SRC, gl::FRAGMENT_SHADER);
    self.shader_program = link_program(vs, fs);
    gl::UseProgram(self.shader_program);
  }

  /// Makes some basic textures in the world.
  pub unsafe fn make_textures(&mut self) {
    let instructions = Vec::from_slice([
            "Use WASD to move, and spacebar to jump.",
            "Use the mouse to look around, and click to remove blocks."
        ]);

    let mut y = 0.99;

    for line in instructions.iter() {
      self.textures.push(self.font.sans.red(*line));

      self.texture_triangles.push(
        TextureVertex::square(
          Aabb2 {
            min: Point2 { x: -0.97, y: y - 0.2 },
            max: Point2 { x: 0.0,   y: y       },
          }));
      y -= 0.2;
    }
  }

  pub unsafe fn make_hud(&mut self) {
    let cursor_color = Color4::of_rgba(0.0, 0.0, 0.0, 0.75);

    self.hud_triangles.push(
      ColoredVertex::square(
        Aabb2 {
          min: Point2 { x: -0.02, y: -0.02 },
          max: Point2 { x:  0.02, y:  0.02 },
        }, cursor_color));
  }

  #[inline]
  pub fn is_mouse_pressed(&self, b: piston::mouse::Button) -> bool {
    self.mouse_buttons_pressed.iter().any(|x| { *x == b })
  }

  /// Sets the opengl projection matrix.
  pub unsafe fn set_projection(&mut self, m: &Matrix4<GLfloat>) {
    let var_name = self.scache.convert("proj_matrix").as_ptr();
    let loc = gl::GetUniformLocation(self.shader_program, var_name);
    assert!(loc != -1, "couldn't read matrix");
    gl::UniformMatrix4fv(loc, 1, 0, mem::transmute(m.ptr()));
  }

  #[inline]
  /// Updates the projetion matrix with all our movements.
  pub unsafe fn update_projection(&mut self) {
    time!(&self.timers, "update.projection", || {
      self.set_projection(&(self.fov_matrix * self.rotation_matrix * self.translation_matrix));
    })
  }

  #[inline]
  /// Renders the selection buffer.
  pub fn render_selection(&self) {
    time!(&self.timers, "render.render_selection", || {
      gl::ClearColor(0.0, 0.0, 0.0, 1.0);
      gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
      self.selection_triangles.draw(gl::TRIANGLES);
    })
  }

  /// Returns the id of the entity at the given (x, y) coordinate in the window.
  /// The pixel coordinates are from (0, 0) to (WINDOW_WIDTH, WINDOW_HEIGHT).
  unsafe fn block_at_window(&self, x: i32, y: i32) -> Option<(u32, uint)> {
    self.render_selection();

    let pixels: Color4<u8> = Color4::of_rgba(0, 0, 0, 0);
    gl::ReadPixels(x, y, 1, 1, gl::RGB, gl::UNSIGNED_BYTE, mem::transmute(&pixels));

    let selection_id = (pixels.r as u32 << 16) | (pixels.g as u32 << 8) | (pixels.b as u32 << 0);
    if selection_id == 0 {
    None
    } else {
    Some((selection_id / 6, selection_id as uint % 6))
    }
  }

  /// Returns (block id, block face) shown at the center of the window.
  unsafe fn block_at_window_center(&self) -> Option<(u32, uint)> {
    self.block_at_window(WINDOW_WIDTH as i32 / 2, WINDOW_HEIGHT as i32 / 2)
  }

  /// Find a collision with self.physics.
  fn world_collision(&self, b: &BoundingBox, self_id: u32) -> Option<Intersect> {
    for (&id, bounds) in self.physics.iter() {
      if id != self_id {
        let i = BoundingBox::intersect(b, bounds);
        match i {
          None => { },
          Some(_) => { return i; },
        }
      }
    }

    None
  }

  unsafe fn place_block(&mut self, low_corner: Vector3<GLfloat>, high_corner: Vector3<GLfloat>, block_type: BlockType, check_collisions: bool) {
    time!(&self.timers, "place_block", || {
      let block = Block {
        block_type: block_type,
        id: self.next_block_id,
      };
      let bounds = BoundingBox {
        low_corner: low_corner,
        high_corner: high_corner,
      };
      let player_bounds = expect_id(self.physics.find(&self.player.id));
      let collided = check_collisions &&
            ( self.world_collision(&bounds, 0).is_some() || 
              BoundingBox::intersect(&bounds, player_bounds).is_some()
            );

      if !collided {
        self.physics.insert(block.id, bounds);
        self.blocks.insert(block.id, block);
        self.index_to_id.push(block.id);
        self.id_to_index.insert(block.id, self.index_to_id.len() - 1);
        self.next_block_id += 1;
      }
    })
  }

  unsafe fn remove_block(&mut self, block_id: u32) {
    // block that will be swapped into block_index in GL buffers after removal
    let block_index = *expect_id(self.id_to_index.find(&block_id));
    let swapped_block_id = self.index_to_id[self.index_to_id.len() - 1];
    self.index_to_id.swap_remove(block_index).expect("ran out of blocks");
    self.blocks.remove(&block_id);
    self.physics.remove(&block_id);
    self.world_triangles.swap_remove(TRIANGLE_VERTICES_PER_BOX, block_index);
    self.outlines.swap_remove(LINE_VERTICES_PER_BOX, block_index);
    self.selection_triangles.swap_remove(TRIANGLE_VERTICES_PER_BOX, block_index);
    self.id_to_index.remove(&block_id);
    if block_id != swapped_block_id {
      self.id_to_index.insert(swapped_block_id, block_index);
    }
  }

  /// Changes the camera's acceleration by the given `da`.
  pub fn walk(&mut self, da: Vector3<GLfloat>) {
    self.player.accel = self.player.accel + da.mul_s(0.2);
  }

  /// Translates the camera by a vector.
  pub unsafe fn translate(&mut self, v: Vector3<GLfloat>) {
    let mut d_camera_speed : Vector3<GLfloat> = Vector3::new(0.0, 0.0, 0.0);

    let player_bounds = { *expect_id(self.physics.find(&self.player.id)) };
    let new_player_bounds = BoundingBox {
      low_corner: player_bounds.low_corner + v,
      high_corner: player_bounds.high_corner + v,
    };

    let collided = match self.world_collision(&new_player_bounds, self.player.id) {
      None => false,
      Some(stop) => {
        d_camera_speed = v*stop - v;
        true
      },
    };

    self.player.speed = self.player.speed + d_camera_speed;

    if collided {
      if v.y < 0.0 {
        self.player.jump_fuel = MAX_JUMP_FUEL;
      }
    } else {
      self.physics.insert(self.player.id, new_player_bounds);
      self.translation_matrix = self.translation_matrix * translate(-v);
      self.update_projection();

      if v.y < 0.0 {
        self.player.jump_fuel = 0;
      }
    }
  }

  #[inline]
  /// Rotate the player's view about a given vector, by `r` radians.
  pub unsafe fn rotate(&mut self, v: Vector3<GLfloat>, r: angle::Rad<GLfloat>) {
    self.rotation_matrix = self.rotation_matrix * from_axis_angle(v, -r);
    self.update_projection();
  }

  #[inline]
  /// Rotate the camera around the y axis, by `r` radians. Positive is
  /// counterclockwise.
  pub unsafe fn rotate_lateral(&mut self, r: angle::Rad<GLfloat>) {
    self.lateral_rotation = self.lateral_rotation + r;
    self.rotate(Vector3::unit_y(), r);
  }

  /// Changes the camera pitch by `r` radians. Positive is up.
  /// Angles that "flip around" (i.e. looking too far up or down)
  /// are sliently rejected.
  pub unsafe fn rotate_vertical(&mut self, r: angle::Rad<GLfloat>) {
    let new_rotation = self.vertical_rotation + r;

    if new_rotation < -angle::Rad::turn_div_4()
    || new_rotation >  angle::Rad::turn_div_4() {
      return
    }

    self.vertical_rotation = new_rotation;
    let axis = self.right();
    self.rotate(axis, r);
  }

  // axes

  /// Return the "right" axis (i.e. the x-axis rotated to match you).
  pub fn right(&self) -> Vector3<GLfloat> {
    return Matrix3::from_axis_angle(&Vector3::unit_y(), self.lateral_rotation).mul_v(&Vector3::unit_x());
  }

  /// Return the "forward" axis (i.e. the z-axis rotated to match you).
  #[allow(dead_code)]
  pub fn forward(&self) -> Vector3<GLfloat> {
    return Matrix3::from_axis_angle(&Vector3::unit_y(), self.lateral_rotation).mul_v(&-Vector3::unit_z());
  }
}

// TODO(cgabeel): This should be removed when rustc bug #8861 is patched.
#[unsafe_destructor]
impl Drop for App {
  fn drop(&mut self) {
    if self.textures.len() == 0 { return }
    unsafe { gl::DeleteTextures(self.textures.len() as i32, &self.textures[0]); }
  }
}

// Shader sources
static VS_SRC: &'static str =
r"#version 330 core
uniform mat4 proj_matrix;

in  vec3 position;
in  vec4 in_color;
out vec4 color;

void main() {
  gl_Position = proj_matrix * vec4(position, 1.0);
  color = in_color;
}";

static FS_SRC: &'static str =
r"#version 330 core
in  vec4 color;
out vec4 frag_color;
void main() {
  frag_color = color;
}";

static ID_VS_SRC: &'static str =
r"#version 330 core
in  vec2 position;
in  vec2 texture_position;
out vec2 tex_position;
void main() {
  tex_position = texture_position;
  gl_Position = vec4(position, -1.0, 1.0);
}";

static TX_SRC: &'static str =
r"#version 330 core
in  vec2 tex_position;
out vec4 frag_color;

uniform sampler2D texture_in;

void main(){
  frag_color = texture(texture_in, vec2(tex_position.x, 1.0 - tex_position.y));
}
";

fn compile_shader(src: &str, ty: GLenum) -> GLuint {
    let shader = gl::CreateShader(ty);
    unsafe {
        // Attempt to compile the shader
        src.with_c_str(|ptr| gl::ShaderSource(shader, 1, &ptr, ptr::null()));
        gl::CompileShader(shader);

        // Get the compile status
        let mut status = gl::FALSE as GLint;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut status);

        // Fail on error
        if status != (gl::TRUE as GLint) {
            let mut len = 0;
            gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = Vec::from_elem(len as uint - 1, 0u8); // subtract 1 to skip the trailing null character
            gl::GetShaderInfoLog(shader, len, ptr::mut_null(), buf.as_mut_ptr() as *mut GLchar);
            fail!("{}", str::from_utf8(buf.slice(0, buf.len())).expect("ShaderInfoLog not valid utf8"));
        }
    }
    shader
}

fn link_program(vs: GLuint, fs: GLuint) -> GLuint {
    let program = gl::CreateProgram();

    gl::AttachShader(program, vs);
    gl::AttachShader(program, fs);
    gl::LinkProgram(program);

    unsafe {
        // Get the link status
        let mut status = gl::FALSE as GLint;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut status);

        // Fail on error
        if status != (gl::TRUE as GLint) {
            let mut len: GLint = 0;
            gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = Vec::from_elem(len as uint - 1, 0u8); // subtract 1 to skip the trailing null character
            gl::GetProgramInfoLog(program, len, ptr::mut_null(), buf.as_mut_ptr() as *mut GLchar);
            fail!("{}", str::from_utf8(buf.slice(0, buf.len())).expect("ProgramInfoLog not valid utf8"));
        }
    }

    program
}

#[allow(dead_code)]
unsafe fn println_c_str(str: *const u8) {
  let mut str = str;
  loop {
    let c = *str as char;
    if c == '\0' {
      println!("");
      return;
    }
    print!("{:c}", c);
    str = str.offset(1);
  }
}

pub fn main() {
  println!("starting");

  let mut window = GameWindowSDL2::new(
    GameWindowSettings {
      title: "playform".to_string(),
      size: [WINDOW_WIDTH, WINDOW_HEIGHT],
      fullscreen: false,
      exit_on_esc: false,
    }
  );

  let opengl_version = gl::GetString(gl::VERSION);
  let glsl_version = gl::GetString(gl::SHADING_LANGUAGE_VERSION);
  print!("OpenGL version: ");
  unsafe { println_c_str(opengl_version); }
  print!("GLSL version: ");
  unsafe { println_c_str(glsl_version); }
  println!("");

  let mut app = unsafe { App::new() };
  app.run(&mut window, &GameIteratorSettings {
    updates_per_second: 30,
    max_frames_per_second: 60,
  });

  println!("finished!");
  println!("");
  println!("runtime stats:");

  app.timers.print();
}
