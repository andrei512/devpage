use super::*;

#[test]
fn a_large_image_file_is_resampled_once_to_fit() {
    let mut data = Vec::new();
    let mut e = png::Encoder::new(&mut data, 9000, 10);
    e.set_color(png::ColorType::Rgba);
    let mut w = e.write_header().unwrap();
    // Red to x = 4500, blue past it.
    let row: Vec<u8> = (0..9000)
        .flat_map(|x| {
            if x <= 4500 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        })
        .collect();
    w.write_image_data(&row.repeat(10)).unwrap();
    drop(w);
    let file = ImageFile::new(data).unwrap();
    assert_eq!((file.width, file.height), (9000, 10));
    assert_eq!((file.pixels_width, file.pixels_height), (4096, 5));
    let px = |x: usize| &file.pixels[4 * x..][..4];
    assert_eq!(px(0), [255, 0, 0, 255]);
    assert_eq!(px(4095), [0, 0, 255, 255]);
    // Column 2048 covers x = 4500 to 4502.2: one part red, 1.2 blue.
    assert_eq!(px(2048), [116, 0, 139, 255]);
    // A fitting file is drawn from its own pixels.
    let (same, w, h) = fit(&[1, 2, 3, 4], 1, 1, MAX_IMAGE_SIDE);
    assert_eq!((same, w, h), (vec![1, 2, 3, 4], 1, 1));
    // A transparent pixel lends no color to its neighbour.
    let (mixed, _, _) = fit(&[255, 255, 255, 0, 0, 0, 255, 255], 2, 1, 1);
    assert_eq!(mixed, [0, 0, 255, 128]);
}
