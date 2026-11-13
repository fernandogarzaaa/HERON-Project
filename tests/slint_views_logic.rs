// Compile and exercise the Views lane's independent data/behavior model in
// ordinary CI. These files are additive until the Slint integration lands;
// without this test target, green CI does not even type-check views.rs.
#[path = "../src/ui/views.rs"]
mod views;
