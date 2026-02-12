use gsplat_core::RenderMode;
use gsplat_render_wgpu::Renderer;

fn main() {
    let mut renderer = Renderer::new(RenderMode::SortedAlpha).expect("renderer init");
    let stats = renderer.render_placeholder();
    println!("desktop-dev ready, frame_ms={}", stats.frame_ms);
}
