#![cfg(feature = "mppa")]
use telamon_kernels::{linalg, Kernel};
use telamon_mppa as mppa;

macro_rules! test_output {
    ($name:ident, $kernel:ty, $num_tests:expr, $params:expr) => {
        #[test]
        fn $name() {
            let _ = env_logger::try_init();
            let mut context = mppa::Context::new();
            <$kernel>::test_correctness($params, $num_tests, &mut context);
        }
    };
}

test_output!(axpy, linalg::Axpy<f32>, 100, (1 << 16, true));
test_output!(mv, linalg::MatVec<f32>, 100, (1 << 4, 1 << 2, true));
test_output!(gesummv, linalg::Gesummv<f32>, 100, (1 << 4, 1 << 4, true));
test_output!(
    fused_mm_identity,
    linalg::FusedMM<f32>,
    100,
    linalg::FusedMMP::new(16, 16, 16)
);
test_output!(
    fused_mm_relu,
    linalg::FusedMM<f32>,
    100,
    linalg::FusedMMP::new(16, 16, 16).activation_fun(linalg::ActivationFunction::ReLU)
);
test_output!(
    fused_mm_sigmoid,
    linalg::FusedMM<f32>,
    100,
    linalg::FusedMMP::new(16, 16, 16).activation_fun(linalg::ActivationFunction::Sigmoid)
);
