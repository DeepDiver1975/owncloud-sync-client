fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let svg_path = "assets/owncloud-icon.svg";
    println!("cargo:rerun-if-changed={svg_path}");

    let svg_data = std::fs::read_to_string(svg_path)
        .unwrap_or_else(|e| panic!("failed to read {svg_path}: {e}"));

    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&svg_data, &opt)
        .unwrap_or_else(|e| panic!("failed to parse SVG: {e}"));

    let size = 16u32;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)
        .expect("failed to allocate pixmap");

    let scale = f32::min(
        size as f32 / tree.size().width(),
        size as f32 / tree.size().height(),
    );
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let out_path = std::path::Path::new(&out_dir).join("owncloud-icon-16.png");
    let file = std::fs::File::create(&out_path)
        .unwrap_or_else(|e| panic!("failed to create output PNG: {e}"));

    let mut encoder = png::Encoder::new(file, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()
        .unwrap_or_else(|e| panic!("failed to write PNG header: {e}"));
    writer.write_image_data(pixmap.data())
        .unwrap_or_else(|e| panic!("failed to write PNG data: {e}"));
}
