use failure::Error;
use overlay::Overlay;

fn main() -> Result<(), Error> {
    let mut overlay = Overlay::new("D:\\Temp\\overlay\\output");
    overlay.add_input("D:\\Temp\\overlay\\input1", 0);
    overlay.add_input("D:\\Temp\\overlay\\input2", 10);
    overlay.process_loop()?;
    Ok(())
}
