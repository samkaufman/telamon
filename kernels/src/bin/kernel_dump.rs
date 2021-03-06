use log::warn;
use std::{fs, path::Path};
use telamon_kernels::{compose::ActivationFunction, linalg, Kernel};
use telamon_x86 as x86;

macro_rules! kernel_dump {
    ($kernel:ty, $dump_ident:ident, $num_tests:expr, $params:expr) => {
        let _ = env_logger::try_init();
        let mut context = x86::Context::default();
        let path = format!("kernel_dump/x86/{}.json", stringify!($dump_ident));
        if !Path::new(&path).exists() {
            let mut file = fs::File::create(path).unwrap();
            <$kernel>::generate_dump($params, &mut context, &mut file);
        } else {
            warn!(
                "Skipping generation of {} as a dump already exists",
                <$kernel>::name()
            );
        }
    };
}

fn main() {
    kernel_dump!(linalg::Axpy<f32>, axpy, 100, (1 << 16, true));
    kernel_dump!(linalg::MatVec<f32>, mv, 100, (1 << 4, 1 << 2, true));
    kernel_dump!(linalg::Gesummv<f32>, gesummv, 100, (1 << 4, 1 << 4, true));
    kernel_dump!(
        linalg::FusedMM<f32>,
        fused_mm_identity,
        100,
        linalg::FusedMMP::new(16, 16, 16)
    );
    kernel_dump!(
        linalg::FusedMM<f32>,
        fused_mm_relu,
        100,
        linalg::FusedMMP::new(16, 16, 16).activation_fun(ActivationFunction::ReLU)
    );
    kernel_dump!(
        linalg::FusedMM<f32>,
        fused_mm_sigmoid,
        100,
        linalg::FusedMMP::new(16, 16, 16).activation_fun(ActivationFunction::Sigmoid)
    );
}
