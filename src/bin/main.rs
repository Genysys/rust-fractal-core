use std::time::Instant;
use rust_fractal::renderer::ImageRenderer;

fn main() {
    println!("Mandelbrot Renderer");
    let center = (
        "-1.99996619445037030418434688506350579675531241540724851511761922944801584242342684381376129778868913812287046406560949864353810575744772166485672496092803920095332",
        "+0.00000000000000000000000000000000030013824367909383240724973039775924987346831190773335270174257280120474975614823581185647299288414075519224186504978181625478529");
    let zoom = 2.3620330788506154104770818136626E157;

//    let center = (
//        "0.0",
//        "0.0");
//    let zoom = 100.0;

//        let center = (
//        "-0.749999987350877481864108888020013802837969258626230419972587823828734338471228477079750588709551510361714463695461745528645748607681279674273355384334270208362211787387351792878073779449767292692440",
//        "0.001000038688236832013124581230049849132759425863378894883003211011278068229551274712347955044740933397589760194545872789087012331273586364914484522575986336846199522726507205442204060303594956029930");
//    let zoom = 1.7E191;

//    let center = (
//        "-1.689573325432279612075987864633746594591438093139394112928000260",
//        "0.000000000000000000000000000000000145514706909258179374258");
//    let zoom = 1.2980742146337048E25;

    let mut renderer = ImageRenderer::new(
        1000,
        1000,
        zoom,
        100000,
        center.0,
        center.1,
        1000,
        0.001,
        false
    );

    let time = Instant::now();
    renderer.render();
    println!("{:<10}{:>6} ms", "TOTAL", time.elapsed().as_millis());
}