#![macro_escape]

use gl::types::*;
use glw::color::Color4;
use glw::vertex::{ColoredVertex};
use nalgebra::Pnt3;
use ncollide::bounding_volume::aabb::AABB;

pub const WINDOW_WIDTH:  uint = 800;
pub const WINDOW_HEIGHT: uint = 600;

pub const TRIANGLES_PER_BOX: uint = 12;
pub const VERTICES_PER_TRIANGLE: uint = 3;
pub const TRIANGLE_VERTICES_PER_BOX: uint = TRIANGLES_PER_BOX * VERTICES_PER_TRIANGLE;

pub const VERTICES_PER_LINE: uint = 2;
pub const LINES_PER_BOX: uint = 12;
pub const LINE_VERTICES_PER_BOX: uint = LINES_PER_BOX * VERTICES_PER_LINE;

pub const USE_LIGHTING: bool = true;

pub const MAX_WORLD_SIZE: uint = 800000;

pub fn partial_min_by<A: Copy, T: Iterator<A>, B: PartialOrd>(t: T, f: |A| -> B) -> Vec<A> {
  let mut t = t;
  let mut min_a = Vec::new();
  let mut min_b = {
    match t.next() {
      None => return min_a,
      Some(a) => {
        min_a.push(a);
        f(a)
      }
    }
  };
  for a in t {
    let b = f(a);
    if b < min_b {
      min_a = Vec::new();
      min_a.push(a);
      min_b = b;
    } else if b == min_b {
      min_a.push(a);
    }
  }

  min_a
}

pub fn to_outlines<'a>(bounds: &AABB) -> [ColoredVertex, ..LINE_VERTICES_PER_BOX] {
  let (x1, y1, z1) = (bounds.mins().x, bounds.mins().y, bounds.mins().z);
  let (x2, y2, z2) = (bounds.maxs().x, bounds.maxs().y, bounds.maxs().z);
  let c = Color4::of_rgba(0.0, 0.0, 0.0, 0.1);

  let vtx = |x: f32, y: f32, z: f32| -> ColoredVertex {
    ColoredVertex {
      position: Pnt3::new(x, y, z),
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

pub fn to_triangles(bounds: &AABB, c: &Color4<GLfloat>) -> [ColoredVertex, ..VERTICES_PER_TRIANGLE * TRIANGLES_PER_BOX] {
  let (x1, y1, z1) = (bounds.mins().x, bounds.mins().y, bounds.mins().z);
  let (x2, y2, z2) = (bounds.maxs().x, bounds.maxs().y, bounds.maxs().z);

  let vtx = |x, y, z| {
    ColoredVertex {
      position: Pnt3::new(x, y, z),
      color: c.clone(),
    }
  };

  // Remember: x increases to the right, y increases up, and z becomes more
  // negative as depth from the viewer increases.
  [
    // front
    vtx(x1, y1, z2), vtx(x2, y2, z2), vtx(x1, y2, z2),
    vtx(x1, y1, z2), vtx(x2, y1, z2), vtx(x2, y2, z2),
    // left
    vtx(x1, y1, z1), vtx(x1, y2, z2), vtx(x1, y2, z1),
    vtx(x1, y1, z1), vtx(x1, y1, z2), vtx(x1, y2, z2),
    // top
    vtx(x1, y2, z1), vtx(x2, y2, z2), vtx(x2, y2, z1),
    vtx(x1, y2, z1), vtx(x1, y2, z2), vtx(x2, y2, z2),
    // back
    vtx(x1, y1, z1), vtx(x2, y2, z1), vtx(x2, y1, z1),
    vtx(x1, y1, z1), vtx(x1, y2, z1), vtx(x2, y2, z1),
    // right
    vtx(x2, y1, z1), vtx(x2, y2, z2), vtx(x2, y1, z2),
    vtx(x2, y1, z1), vtx(x2, y2, z1), vtx(x2, y2, z2),
    // bottom
    vtx(x1, y1, z1), vtx(x2, y1, z2), vtx(x1, y1, z2),
    vtx(x1, y1, z1), vtx(x2, y1, z1), vtx(x2, y1, z2),
  ]
}
