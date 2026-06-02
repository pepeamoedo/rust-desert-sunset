use rust_desert_sunset::run;

fn main() {
    env_logger::init();
    pollster::block_on(run());
}
