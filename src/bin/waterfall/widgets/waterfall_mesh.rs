//! Ring-buffer waterfall mesh: fixed screen rows, UV maps texture ring via `row_head`.

use eframe::egui::{Mesh, Rect, TextureId, emath::pos2};
use eframe::epaint::Vertex;

/// Build a textured mesh: screen row `s` (0 = top / newest) samples texture row
/// `(row_head + row_count - 1 - s) % row_count`.
pub fn build_waterfall_ring_mesh(
    rect: Rect,
    row_head: usize,
    row_count: usize,
    texture_id: TextureId,
) -> Mesh {
    let h = row_count.max(1);
    let hf = h as f32;
    let head = row_head % h;
    let mut mesh = Mesh::with_texture(texture_id);
    let left = rect.left();
    let right = rect.right();
    let top = rect.top();
    let height = rect.height().max(1.0);

    for s in 0..h {
        let tex_row = (head + h - 1 - s) % h;
        let y0 = top + height * s as f32 / hf;
        let y1 = top + height * (s + 1) as f32 / hf;
        let v0 = tex_row as f32 / hf;
        let v1 = (tex_row + 1) as f32 / hf;
        let i = mesh.vertices.len() as u32;
        mesh.vertices.push(Vertex {
            pos: pos2(left, y0),
            uv: pos2(0.0, v0),
            color: eframe::egui::Color32::WHITE,
        });
        mesh.vertices.push(Vertex {
            pos: pos2(right, y0),
            uv: pos2(1.0, v0),
            color: eframe::egui::Color32::WHITE,
        });
        mesh.vertices.push(Vertex {
            pos: pos2(right, y1),
            uv: pos2(1.0, v1),
            color: eframe::egui::Color32::WHITE,
        });
        mesh.vertices.push(Vertex {
            pos: pos2(left, y1),
            uv: pos2(0.0, v1),
            color: eframe::egui::Color32::WHITE,
        });
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, TextureId};

    #[test]
    fn top_screen_row_uses_newest_texture_row() {
        let h = 360usize;
        let rect = Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(100.0, h as f32));
        let head = 0usize;
        let mesh = build_waterfall_ring_mesh(rect, head, h, TextureId::default());
        let top = &mesh.vertices[0];
        let expected_v = (h - 1) as f32 / h as f32;
        assert!((top.uv.y - expected_v).abs() < 1e-5, "top v={}", top.uv.y);
    }

    #[test]
    fn head_advances_newest_row() {
        let h = 360usize;
        let rect = Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(100.0, h as f32));
        let mesh = build_waterfall_ring_mesh(rect, 1, h, TextureId::default());
        assert!((mesh.vertices[0].uv.y - 0.0).abs() < 1e-5);
    }
}
