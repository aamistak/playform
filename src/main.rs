#![feature(globs)] // Allow global imports
#![feature(macro_rules)]

extern crate cgmath;
extern crate gl;
extern crate piston;
extern crate sdl2;
extern crate sdl2_game_window;

use color::Color4;
use cgmath::angle;
use cgmath::array::Array2;
use cgmath::matrix::{Matrix, Matrix3, Matrix4};
use cgmath::num::{BaseFloat};
use cgmath::vector::{Vector, Vector2, Vector3, Vector4};
use cgmath::projection;
use piston::*;
use gl::types::*;
use sdl2_game_window::GameWindowSDL2;
use sdl2::mouse;
use std::mem;
use std::iter::range_inclusive;
use std::ptr;
use std::str;
use std::num;

mod color;
mod fontloader;
mod stopwatch;
mod ttf;

// TODO(cgaebel): How the hell do I get this to be exported from `mod stopwatch`?
macro_rules! time(
  ($timers:expr, $name:expr, $f:expr) => (
    unsafe { ($timers as *const stopwatch::TimerSet).to_option() }.unwrap().time($name, $f)
  );
)

static WINDOW_WIDTH: u32 = 800;
static WINDOW_HEIGHT: u32 = 600;

static TRIANGLES_PER_BLOCK: uint = 12;
static LINES_PER_BLOCK: uint = 12;
static VERTICES_PER_TRIANGLE: uint = 3;
static VERTICES_PER_LINE: uint = 2;
static TRIANGLE_VERTICES_PER_BLOCK: uint = TRIANGLES_PER_BLOCK * VERTICES_PER_TRIANGLE;
static LINE_VERTICES_PER_BLOCK: uint = LINES_PER_BLOCK * VERTICES_PER_LINE;

static MAX_JUMP_FUEL: uint = 4;

#[deriving(Clone)]
// Rendering vertex: position and color.
pub struct Vertex {
  position: Vector3<GLfloat>,
  color:    Color4<GLfloat>,
}

impl Vertex {
  fn new(x: GLfloat, y: GLfloat, z: GLfloat, c: Color4<GLfloat>) -> Vertex {
    Vertex {
      position: Vector3::new(x, y, z),
      color:    c,
    }
  }
}

#[deriving(Clone)]
pub struct TextureVertex {
  position: Vector2<GLfloat>,
  texture_position: Vector2<GLfloat>,
}

impl TextureVertex {
  #[inline]
  pub fn new(x: GLfloat, y: GLfloat, tx: GLfloat, ty: GLfloat) -> TextureVertex {
    TextureVertex {
      position: Vector2::new(x, y),
      texture_position: Vector2::new(tx, ty),
    }
  }
}

pub struct VertexAttribData<'a> {
  name: &'a str,
  span: uint,
}

impl<'a> VertexAttribData<'a> {
  pub fn new(name: &'a str, span: uint) -> VertexAttribData<'a> {
    VertexAttribData {
      name: name,
      span: span,
    }
  }
}

pub struct GLBuffer<T> {
  vertex_array: u32,
  vertex_buffer: u32,
  length: uint,
  capacity: uint,
}

impl<T: Clone> GLBuffer<T> {
  #[inline]
  pub unsafe fn null() -> GLBuffer<T> {
    GLBuffer {
      vertex_array: -1 as u32,
      vertex_buffer: -1 as u32,
      length: 0,
      capacity: 0,
    }
  }

  #[inline]
  pub unsafe fn new(shader_program: GLuint, attribs: &[VertexAttribData], capacity: uint) -> GLBuffer<T> {
    let mut vertex_array = 0;
    let mut vertex_buffer = 0;
    gl::GenVertexArrays(1, &mut vertex_array);
    gl::GenBuffers(1, &mut vertex_buffer);

    gl::BindVertexArray(vertex_array);
    gl::BindBuffer(gl::ARRAY_BUFFER, vertex_buffer);

    let mut offset = 0;
    for attrib in attribs.iter() {
      let shader_attrib = glGetAttribLocation(shader_program, attrib.name) as GLuint;
      if shader_attrib == -1 {
        fail!("shader attribute \"{}\" not found", attrib.name);
      }

      gl::EnableVertexAttribArray(shader_attrib);
      gl::VertexAttribPointer(
          shader_attrib,
          attrib.span as i32,
          gl::FLOAT,
          gl::FALSE as GLboolean,
          mem::size_of::<T>() as i32,
          ptr::null().offset(offset),
      );
      offset += (attrib.span * mem::size_of::<GLfloat>()) as int;
    }

    if offset != mem::size_of::<T>() as int {
      fail!("attribs are incorrectly sized!");
    }

    gl::BufferData(
      gl::ARRAY_BUFFER,
      (capacity * mem::size_of::<T>()) as GLsizeiptr,
      ptr::null(),
      gl::DYNAMIC_DRAW,
    );

    GLBuffer {
      vertex_array: vertex_array,
      vertex_buffer: vertex_buffer,
      length: 0,
      capacity: capacity,
    }
  }

  pub unsafe fn swap_remove(&mut self, span: uint, i: uint) {
    gl::BindVertexArray(self.vertex_array);
    gl::BindBuffer(gl::ARRAY_BUFFER, self.vertex_buffer);

    self.length -= span;
    let size = mem::size_of::<T>();
    let copy_size = (size * span) as uint;
    let mut bytes: Vec<u8> = Vec::with_capacity(copy_size);
    bytes.set_len(copy_size);
    gl::GetBufferSubData(
      gl::ARRAY_BUFFER,
      (self.length * size) as i64,
      copy_size as i64,
      mem::transmute(&bytes.as_mut_slice()[0]),
    );
    gl::BufferSubData(
      gl::ARRAY_BUFFER,
      (i * span * size) as i64,
      copy_size as i64,
      mem::transmute(&bytes.slice(0, bytes.len())[0]),
    );
  }

  #[inline]
  pub unsafe fn push(&mut self, vs: &[T]) {
    if self.length >= self.capacity {
      fail!("Overfilled GLBuffer: {} out of {}", self.length, self.capacity);
    }

    gl::BindVertexArray(self.vertex_array);
    gl::BindBuffer(gl::ARRAY_BUFFER, self.vertex_buffer);

    let size = mem::size_of::<T>() as i64;
    gl::BufferSubData(
      gl::ARRAY_BUFFER,
      size * self.length as i64,
      size * vs.len() as i64,
      mem::transmute(&vs[0]),
    );

    self.length += vs.len();
  }

  #[inline]
  pub fn draw(&self, mode: GLenum) {
    self.draw_slice(mode, 0, self.length);
  }

  pub fn draw_slice(&self, mode: GLenum, start: uint, len: uint) {
    gl::BindVertexArray(self.vertex_array);
    gl::BindBuffer(gl::ARRAY_BUFFER, self.vertex_buffer);

    gl::DrawArrays(mode, start as i32, len as i32);
  }

  #[inline]
  pub fn drop(&self) {
    unsafe {
      gl::DeleteBuffers(1, &self.vertex_buffer);
      gl::DeleteVertexArrays(1, &self.vertex_array);
    }
  }
}

#[deriving(Clone)]
pub enum BlockType {
  Grass,
  Dirt,
  Stone,
}

impl BlockType {
  fn to_color(&self) -> Color4<GLfloat> {
    match *self {
      Grass => Color4::new(0.0, 0.5,  0.0, 1.0),
      Dirt  => Color4::new(0.2, 0.15, 0.1, 1.0),
      Stone => Color4::new(0.5, 0.5,  0.5, 1.0),
    }
  }
}

#[deriving(Clone)]
pub struct Block {
  // bounds of the Block
  low_corner: Vector3<GLfloat>,
  high_corner: Vector3<GLfloat>,
  block_type: BlockType,
}

enum Intersect {
  Intersect(Vector3<GLfloat>),
  NoIntersect,
}

enum Intersect1 {
  Within,
  Partial,
  NoIntersect1,
}

// Find whether two Blocks intersect.
fn intersect(b1: &Block, b2: &Block) -> Intersect {
  fn intersect1(x1l: GLfloat, x1h: GLfloat, x2l: GLfloat, x2h: GLfloat) -> Intersect1 {
    if x1l > x2l && x1h <= x2h {
      Within
    } else if x1h > x2l && x2h > x1l {
      Partial
    } else {
      NoIntersect1
    }
  }

  let mut ret = true;
  let mut v = Vector3::ident();
  match intersect1(b1.low_corner.x, b1.high_corner.x, b2.low_corner.x, b2.high_corner.x) {
    Within => { },
    Partial => { v.x = 0.0; },
    NoIntersect1 => { ret = false; },
  }
  match intersect1(b1.low_corner.y, b1.high_corner.y, b2.low_corner.y, b2.high_corner.y) {
    Within => { },
    Partial => { v.y = 0.0; },
    NoIntersect1 => { ret = false; },
  }
  match intersect1(b1.low_corner.z, b1.high_corner.z, b2.low_corner.z, b2.high_corner.z) {
    Within => { },
    Partial => { v.z = 0.0; },
    NoIntersect1 => { ret = false; },
  }

  if ret {
    Intersect(v)
  } else {
    NoIntersect
  }
}

impl Block {
  fn new(low_corner: Vector3<GLfloat>, high_corner: Vector3<GLfloat>, block_type: BlockType) -> Block {
    Block {
      low_corner: low_corner.clone(),
      high_corner: high_corner.clone(),
      block_type: block_type,
    }
  }

  // Construct the faces of the block as triangles for rendering.
  // Triangle vertices are in clockwise order when viewed from the outside of
  // the cube, for rendering purposes.
  fn to_triangles(&self, c: Color4<GLfloat>) -> [Vertex, ..VERTICES_PER_TRIANGLE * TRIANGLES_PER_BLOCK] {
    let (x1, y1, z1) = (self.low_corner.x, self.low_corner.y, self.low_corner.z);
    let (x2, y2, z2) = (self.high_corner.x, self.high_corner.y, self.high_corner.z);

    let vtx = |x: GLfloat, y: GLfloat, z: GLfloat| -> Vertex {
      Vertex::new(x, y, z, c)
    };

    [
      // front
      vtx(x1, y1, z1), vtx(x1, y2, z1), vtx(x2, y2, z1),
      vtx(x1, y1, z1), vtx(x2, y2, z1), vtx(x2, y1, z1),
      // left
      vtx(x1, y1, z2), vtx(x1, y2, z2), vtx(x1, y2, z1),
      vtx(x1, y1, z2), vtx(x1, y2, z1), vtx(x1, y1, z1),
      // top
      vtx(x1, y2, z1), vtx(x1, y2, z2), vtx(x2, y2, z2),
      vtx(x1, y2, z1), vtx(x2, y2, z2), vtx(x2, y2, z1),
      // back
      vtx(x2, y1, z2), vtx(x2, y2, z2), vtx(x1, y2, z2),
      vtx(x2, y1, z2), vtx(x1, y2, z2), vtx(x1, y1, z2),
      // right
      vtx(x2, y1, z1), vtx(x2, y2, z1), vtx(x2, y2, z2),
      vtx(x2, y1, z1), vtx(x2, y2, z2), vtx(x2, y1, z2),
      // bottom
      vtx(x1, y1, z2), vtx(x1, y1, z1), vtx(x2, y1, z1),
      vtx(x1, y1, z2), vtx(x2, y1, z1), vtx(x2, y1, z2),
    ]
  }

  #[inline]
  fn to_colored_triangles(&self) -> [Vertex, ..VERTICES_PER_TRIANGLE * TRIANGLES_PER_BLOCK] {
    self.to_triangles(self.block_type.to_color())
  }

  // Construct outlines for this Block, to sharpen the edges.
  fn to_outlines(&self) -> [Vertex, ..VERTICES_PER_LINE * LINES_PER_BLOCK] {
    // distance from the block to construct the bounding outlines.
    let d = 0.002;
    let (x1, y1, z1) = (self.low_corner.x - d, self.low_corner.y - d, self.low_corner.z - d);
    let (x2, y2, z2) = (self.high_corner.x + d, self.high_corner.y + d, self.high_corner.z + d);
    let c = Color4::new(0.0, 0.0, 0.0, 1.0);

    fn vtx(x: GLfloat, y: GLfloat, z: GLfloat, a: Color4<GLfloat>) -> Vertex {
      Vertex::new(x, y, z, a)
    }

    [
      vtx(x1, y1, z1, c), vtx(x2, y1, z1, c),
      vtx(x1, y2, z1, c), vtx(x2, y2, z1, c),
      vtx(x1, y1, z2, c), vtx(x2, y1, z2, c),
      vtx(x1, y2, z2, c), vtx(x2, y2, z2, c),

      vtx(x1, y1, z1, c), vtx(x1, y2, z1, c),
      vtx(x2, y1, z1, c), vtx(x2, y2, z1, c),
      vtx(x1, y1, z2, c), vtx(x1, y2, z2, c),
      vtx(x2, y1, z2, c), vtx(x2, y2, z2, c),

      vtx(x1, y1, z1, c), vtx(x1, y1, z2, c),
      vtx(x2, y1, z1, c), vtx(x2, y1, z2, c),
      vtx(x1, y2, z1, c), vtx(x1, y2, z2, c),
      vtx(x2, y2, z1, c), vtx(x2, y2, z2, c),
    ]
  }

  pub fn contains_point(&self, point: Vector3<GLfloat>) -> bool {
    (self.low_corner.x <= point.x) && (point.x <= self.high_corner.x) &&
    (self.low_corner.y <= point.y) && (point.y <= self.high_corner.y) &&
    (self.low_corner.z <= point.z) && (point.z <= self.high_corner.z)
  }
}

pub struct App {
  world_data: Vec<Block>,
  // position; units are world coordinates
  camera_position: Vector3<GLfloat>,
  // speed; units are world coordinates
  camera_speed: Vector3<GLfloat>,
  // acceleration; x/z units are relative to player facing
  camera_accel: Vector3<GLfloat>,
  // this is depleted as we jump and replenished as we stand.
  jump_fuel: uint,
  // are we currently trying to jump? (e.g. holding the key).
  jumping: bool,
  // OpenGL buffers
  world_triangles: GLBuffer<Vertex>,
  outlines: GLBuffer<Vertex>,
  hud_triangles: GLBuffer<Vertex>,
  texture_triangles: GLBuffer<TextureVertex>,
  textures: Vec<GLuint>,
  // OpenGL-friendly equivalent of world_data for selection/picking.
  selection_triangles: GLBuffer<Vertex>,
  // OpenGL projection matrix components
  hud_matrix: Matrix4<GLfloat>,
  fov_matrix: Matrix4<GLfloat>,
  translation_matrix: Matrix4<GLfloat>,
  rotation_matrix: Matrix4<GLfloat>,
  lateral_rotation: angle::Rad<GLfloat>,
  // OpenGL shader "program" id.
  shader_program: u32,
  texture_shader: u32,

  // Is LMB pressed?
  is_mouse_pressed: bool,

  font: fontloader::FontLoader,

  timers: stopwatch::TimerSet,
}

// Create a 3D translation matrix.
pub fn translate(t: Vector3<GLfloat>) -> Matrix4<GLfloat> {
  Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    t.x, t.y, t.z, 1.0,
  )
}

// Create a 3D perspective initialization matrix.
pub fn perspective(fovy: GLfloat, aspect: GLfloat, near: GLfloat, far: GLfloat) -> Matrix4<GLfloat> {
  Matrix4::new(
    fovy / aspect, 0.0, 0.0,                              0.0,
    0.0,          fovy, 0.0,                              0.0,
    0.0,           0.0, (near + far) / (near - far),     -1.0,
    0.0,           0.0, 2.0 * near * far / (near - far),  0.0,
  )
}

#[inline]
// Create a XY symmetric ortho matrix.
pub fn sortho(dx: GLfloat, dy: GLfloat, near: GLfloat, far: GLfloat) -> Matrix4<GLfloat> {
  projection::ortho(-dx, dx, -dy, dy, near, far)
}

// Create a matrix from a rotation around an arbitrary axis
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
pub fn w_normalize<S: Div<S, S> + num::One>(v: Vector4<S>) -> Vector4<S> {
  Vector4::new(
    v.x / v.w,
    v.y / v.w,
    v.z / v.w,
    num::one()
  )
}

#[allow(non_snake_case_functions)]
pub unsafe fn glGetAttribLocation(shader_program: GLuint, name: &str) -> GLint {
  name.with_c_str(|ptr| gl::GetAttribLocation(shader_program, ptr))
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
          if !self.jumping {
            self.jumping = true;
            // this 0.3 is duplicated in a few places
            self.camera_accel.y = self.camera_accel.y + 0.3;
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
          if self.jumping {
            self.jumping = false;
            // this 0.3 is duplicated in a few places
            self.camera_accel.y = self.camera_accel.y - 0.3;
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

  #[inline]
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
      match args.button {
        piston::mouse::Left => {
          self.is_mouse_pressed = true;
        },
        _ => { }
      }
    })
  }

  fn mouse_release(&mut self, _: &mut GameWindowSDL2, args: &MouseReleaseArgs) {
    match args.button {
      piston::mouse::Left => {
        self.is_mouse_pressed = false;
      },
      _ => {}
    }
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
      gl::ClearDepth(10.0);
      gl::ClearColor(0.0, 0.0, 0.0, 1.0);

      unsafe {
        self.set_up_shaders();

        // initialize the projection matrix
        self.fov_matrix = perspective(3.14/3.0, 4.0/3.0, 0.1, 100.0);
        self.translate(Vector3::new(0.0, 4.0, 10.0));
        self.update_projection();
      }

      let timers = &self.timers;

      timers.time("load.construct", || {
        // low dirt block
        for i in range_inclusive(-1i, 1) {
          for j in range_inclusive(-1i, 1) {
            let (x1, y1, z1) = (3.0 + i as GLfloat, 6.0, 0.0 + j as GLfloat);
            let (x2, y2, z2) = (4.0 + i as GLfloat, 7.0, 1.0 + j as GLfloat);
            self.world_data.grow(1, &Block::new(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Dirt));
          }
        }
        // high dirt block
        for i in range_inclusive(-1i, 1) {
          for j in range_inclusive(-1i, 1) {
            let (x1, y1, z1) = (0.0 + i as GLfloat, 12.0, 5.0 + j as GLfloat);
            let (x2, y2, z2) = (1.0 + i as GLfloat, 13.0, 6.0 + j as GLfloat);
            self.world_data.grow(1, &Block::new(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Dirt));
          }
        }
        // ground
        for i in range_inclusive(-32i, 32) {
          for j in range_inclusive(-32i, 32) {
            let (x1, y1, z1) = (i as GLfloat - 0.5, 0.0, j as GLfloat - 0.5);
            let (x2, y2, z2) = (i as GLfloat + 0.5, 1.0, j as GLfloat + 0.5);
            self.world_data.grow(1, &Block::new(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Grass));
          }
        }
        // front wall
        for i in range_inclusive(-32i, 32) {
          for j in range_inclusive(0i, 32) {
            let (x1, y1, z1) = (i as GLfloat - 0.5, 1.0 + j as GLfloat, -32.0 - 0.5);
            let (x2, y2, z2) = (i as GLfloat + 0.5, 2.0 + j as GLfloat, -32.0 + 0.5);
            self.world_data.grow(1, &Block::new(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone));
          }
        }
        // back wall
        for i in range_inclusive(-32i, 32) {
          for j in range_inclusive(0i, 32) {
            let (x1, y1, z1) = (i as GLfloat - 0.5, 1.0 + j as GLfloat, 32.0 - 0.5);
            let (x2, y2, z2) = (i as GLfloat + 0.5, 2.0 + j as GLfloat, 32.0 + 0.5);
            self.world_data.grow(1, &Block::new(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone));
          }
        }
        // left wall
        for i in range_inclusive(-32i, 32) {
          for j in range_inclusive(0i, 32) {
            let (x1, y1, z1) = (-32.0 - 0.5, 1.0 + j as GLfloat, i as GLfloat - 0.5);
            let (x2, y2, z2) = (-32.0 + 0.5, 2.0 + j as GLfloat, i as GLfloat + 0.5);
            self.world_data.grow(1, &Block::new(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone));
          }
        }
        // right wall
        for i in range_inclusive(-32i, 32) {
          for j in range_inclusive(0i, 32) {
            let (x1, y1, z1) = (32.0 - 0.5, 1.0 + j as GLfloat, i as GLfloat - 0.5);
            let (x2, y2, z2) = (32.0 + 0.5, 2.0 + j as GLfloat, i as GLfloat + 0.5);
            self.world_data.grow(1, &Block::new(Vector3::new(x1, y1, z1), Vector3::new(x2, y2, z2), Stone));
          }
        }
      });

      unsafe {
        self.selection_triangles = GLBuffer::new(
          self.shader_program,
          [ VertexAttribData::new("position", 3),
            VertexAttribData::new("in_color", 4),
          ],
          self.world_data.len() * TRIANGLE_VERTICES_PER_BLOCK,
        );

        self.world_triangles = GLBuffer::new(
          self.shader_program,
          [ VertexAttribData::new("position", 3),
            VertexAttribData::new("in_color", 4),
          ],
          self.world_data.len() * TRIANGLE_VERTICES_PER_BLOCK,
        );

        self.outlines = GLBuffer::new(
          self.shader_program,
          [ VertexAttribData::new("position", 3),
            VertexAttribData::new("in_color", 4),
          ],
          self.world_data.len() * LINE_VERTICES_PER_BLOCK,
        );

        self.hud_triangles = GLBuffer::new(
          self.shader_program,
          [ VertexAttribData::new("position", 3),
            VertexAttribData::new("in_color", 4),
          ],
          16 * VERTICES_PER_TRIANGLE,
        );

        self.texture_triangles = GLBuffer::new(
          self.texture_shader,
          [ VertexAttribData::new("position", 2),
            VertexAttribData::new("texture_position", 2),
          ],
          8 * VERTICES_PER_TRIANGLE,
        );

        self.make_textures();
        self.make_world_render_data();
        self.make_hud();
      }
    })
  }

  fn update(&mut self, _: &mut GameWindowSDL2, _: &UpdateArgs) {
    time!(&self.timers, "update", || unsafe {
      if self.jumping {
        if self.jump_fuel > 0 {
          self.jump_fuel -= 1;
        } else {
          // this code is duplicated in a few places
          self.jumping = false;
          self.camera_accel.y = self.camera_accel.y - 0.3;
        }
      }

      let dP = self.camera_speed;
      if dP.x != 0.0 {
        self.translate(Vector3::new(dP.x, 0.0, 0.0));
      }
      if dP.y != 0.0 {
        self.translate(Vector3::new(0.0, dP.y, 0.0));
      }
      if dP.z != 0.0 {
        self.translate(Vector3::new(0.0, 0.0, dP.z));
      }

      let dV = Matrix3::from_axis_angle(&Vector3::unit_y(), self.lateral_rotation).mul_v(&self.camera_accel);
      self.camera_speed = self.camera_speed + dV;
      // friction
      self.camera_speed = self.camera_speed * Vector3::new(0.7, 0.99, 0.7);

      // Block deletion
      if self.is_mouse_pressed {
        time!(&self.timers, "update.delete_block", || unsafe {
          self
            .block_at_screen(WINDOW_WIDTH as f32 / 2.0, WINDOW_HEIGHT as f32 / 2.0)
            .map(|block_index| {
              assert!(block_index < self.world_data.len());
              self.world_data.swap_remove(block_index);
              self.world_triangles.swap_remove(TRIANGLE_VERTICES_PER_BLOCK, block_index);
              self.outlines.swap_remove(LINE_VERTICES_PER_BLOCK, block_index);
              self.selection_triangles.swap_remove(TRIANGLE_VERTICES_PER_BLOCK, block_index);
            });
        })
      }
    })
  }

  fn render(&mut self, _: &mut GameWindowSDL2, _: &RenderArgs) {
    time!(&self.timers, "render", || unsafe {
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

impl App {
  pub unsafe fn new() -> App {
    App {
      world_data: Vec::new(),
      camera_position: Vector3::zero(),
      camera_speed: Vector3::zero(),
      camera_accel: Vector3::new(0.0, -0.1, 0.0),
      jump_fuel: 0,
      jumping: false,
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
      shader_program: -1 as u32,
      texture_shader: -1 as u32,
      is_mouse_pressed: false,
      font: fontloader::FontLoader::new(),
      timers: stopwatch::TimerSet::new(),
    }
  }

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

  pub unsafe fn make_textures(&mut self) {
    let instructions = Vec::from_slice([
            "Use WASD to move, and spacebar to jump.",
            "Use the mouse to look around, and click to remove blocks."
        ]);

    let mut y = 0.99;

    for line in instructions.iter() {
      self.textures.push(self.font.sans.red(*line));

      let (x1, y1) = (-0.97, y - 0.2);
      let (x2, y2) = (0.0, y);
      self.texture_triangles.push([
        TextureVertex::new(x1, y1, 0.0, 0.0),
        TextureVertex::new(x2, y2, 1.0, 1.0),
        TextureVertex::new(x1, y2, 0.0, 1.0),

        TextureVertex::new(x1, y1, 0.0, 0.0),
        TextureVertex::new(x2, y1, 1.0, 0.0),
        TextureVertex::new(x2, y2, 1.0, 1.0),
      ]);
      y -= 0.2;
    }
  }

  // Update the OpenGL vertex data with the world data world_triangles.
  pub unsafe fn make_world_render_data(&mut self) {
    fn selection_color(i: u32) -> Color4<GLfloat> {
      assert!(i < 0xFF000000, "too many items for selection buffer");
      let i = i + 1;
      let ret = Color4::new(
        (mask(0x00FF0000, i) as GLfloat / 255.0),
        (mask(0x0000FF00, i) as GLfloat / 255.0),
        (mask(0x000000FF, i) as GLfloat / 255.0),
        1.0,
      );

      assert!(ret.r >= 0.0);
      assert!(ret.r <= 1.0);
      assert!(ret.g >= 0.0);
      assert!(ret.g <= 1.0);
      assert!(ret.b >= 0.0);
      assert!(ret.b <= 1.0);
      ret
    }

    time!(&self.timers, "render.make_data", || {
      for (i, block) in self.world_data.iter().enumerate() {
        self.world_triangles.push(block.to_colored_triangles());
        self.outlines.push(block.to_outlines());
        self.selection_triangles.push(block.to_triangles(selection_color(i as u32)));
      }
    })
  }

  pub unsafe fn make_hud(&mut self) {
    let cursor_color = Color4::new(0.0, 0.0, 0.0, 0.75);
    self.hud_triangles.push([
      Vertex::new(-0.02, -0.02, 0.0, cursor_color),
      Vertex::new(0.02, 0.02, 0.0, cursor_color),
      Vertex::new(-0.02, 0.02, 0.0, cursor_color),

      Vertex::new(-0.02, -0.02, 0.0, cursor_color),
      Vertex::new(0.02, -0.02, 0.0, cursor_color),
      Vertex::new(0.02, 0.02, 0.0, cursor_color),
    ]);
  }

  pub unsafe fn set_projection(&mut self, m: &Matrix4<GLfloat>) {
    let loc = gl::GetUniformLocation(self.shader_program, "proj_matrix".to_c_str().unwrap());
    if loc == -1 {
      fail!("couldn't read matrix");
    }
    gl::UniformMatrix4fv(loc, 1, 0, mem::transmute(m.ptr()));
  }

  #[inline]
  /// Calculate the projection matrix used for rendering.
  fn projection_matrix(&self) -> Matrix4<GLfloat> {
    self.fov_matrix * self.rotation_matrix * self.translation_matrix
  }

  #[inline]
  /// Translate window coordinates to screen coordinates
  fn to_screen_position(&self, x: GLfloat, y: GLfloat) -> Vector2<GLfloat> {
    Vector2::new(
      x as GLfloat * 2.0 / WINDOW_WIDTH as GLfloat - 1.0,
      1.0 - y as GLfloat * 2.0 / WINDOW_HEIGHT as GLfloat,
    )
  }

  #[inline]
  /// Calculate the world coordinates corresponding to the given screen
  /// coordinates.
  fn unproject(&self, screen_position: Vector3<GLfloat>) -> Vector3<GLfloat> {
    w_normalize(
      self
        .projection_matrix()
        .invert()
        .expect("projection matrix is uninvertible")
        .mul_v(&screen_position.extend(1.0))
    ).truncate()
  }

  #[inline]
  pub unsafe fn update_projection(&mut self) {
    time!(&self.timers, "update.projection", || {
      self.set_projection(&self.projection_matrix());
    })
  }

  #[inline]
  pub fn render_selection(&mut self) {
    time!(&self.timers, "render.render_selection", || {
      gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
      self.selection_triangles.draw(gl::TRIANGLES);
    })
  }

  pub unsafe fn block_at_screen(&mut self, x: GLfloat, y: GLfloat) -> Option<uint> {
    time!(&self.timers, "block_at_screen", || {
      let mut z = 0.0;
      time!(&self.timers, "block_at_screen.read_pixels", || {
        // TODO: should we round instead of (as i32)ing?
        gl::ReadPixels(x as i32, y as i32, 1, 1, gl::DEPTH_COMPONENT, gl::FLOAT, mem::transmute(&z));
      });

      let world_position = time!(&self.timers, "block_at_screen.unproject", || {
        let screen_position = self.to_screen_position(x, y);
        self.unproject(Vector3::new(screen_position.x, screen_position.y, z))
      });

      time!(&self.timers, "block_at_screen.find_block", || {
        // TODO: better iteration
        for i in range(0, self.world_data.len()) {
          if self.world_data[i].contains_point(world_position) {
            return Some(i)
          }
        }
        None
      })
    })
  }

  #[inline]
  pub fn walk(&mut self, da: Vector3<GLfloat>) {
    self.camera_accel = self.camera_accel + da.mul_s(0.2);
  }

  fn construct_player(&self, high_corner: Vector3<GLfloat>) -> Block {
    let low_corner = high_corner - Vector3::new(0.5, 2.0, 1.0);
    // TODO: this really shouldn't be Stone.
    Block::new(low_corner, high_corner, Stone)
  }

  // move the player by a vector
  pub unsafe fn translate(&mut self, v: Vector3<GLfloat>) {
    let player = self.construct_player(self.camera_position + v);

    let mut d_camera_speed : Vector3<GLfloat> = Vector3::new(0.0, 0.0, 0.0);

    let collided =
      self
        .world_data
        .iter()
        .any(|block|
          match intersect(&player, block) {
            Intersect(stop) => {
              d_camera_speed = v*stop - v;
              true
            }
            NoIntersect => false,
          }
        );

    self.camera_speed = self.camera_speed + d_camera_speed;

    if collided {
      if v.y < 0.0 {
        self.jump_fuel = MAX_JUMP_FUEL;
      }
    } else {
      self.camera_position = self.camera_position + v;
      self.translation_matrix = self.translation_matrix * translate(-v);
      self.update_projection();

      if v.y < 0.0 {
        self.jump_fuel = 0;
      }
    }
  }

  #[inline]
  // rotate the player's view.
  pub unsafe fn rotate(&mut self, v: Vector3<GLfloat>, r: angle::Rad<GLfloat>) {
    self.rotation_matrix = self.rotation_matrix * from_axis_angle(v, -r);
    self.update_projection();
  }

  #[inline]
  pub unsafe fn rotate_lateral(&mut self, r: angle::Rad<GLfloat>) {
    self.lateral_rotation = self.lateral_rotation + r;
    self.rotate(Vector3::unit_y(), r);
  }

  #[inline]
  pub unsafe fn rotate_vertical(&mut self, r: angle::Rad<GLfloat>) {
    let axis = self.right();
    self.rotate(axis, r);
  }

  // axes

  // Return the "right" axis (i.e. the x-axis rotated to match you).
  pub fn right(&self) -> Vector3<GLfloat> {
    return Matrix3::from_axis_angle(&Vector3::unit_y(), self.lateral_rotation).mul_v(&Vector3::unit_x());
  }

  // Return the "forward" axis (i.e. the z-axis rotated to match you).
  pub fn forward(&self) -> Vector3<GLfloat> {
    return Matrix3::from_axis_angle(&Vector3::unit_y(), self.lateral_rotation).mul_v(&-Vector3::unit_z());
  }

  pub unsafe fn drop(&mut self) {
    if self.textures.len() > 0 {
      gl::DeleteTextures(self.textures.len() as i32, &self.textures[0]);
    }
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

#[allow(dead_code)]
fn main() {
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
