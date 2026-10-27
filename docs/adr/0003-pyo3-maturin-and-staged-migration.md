# PyO3、maturin 与分阶段迁移

目标工程采用 Rust workspace，将领域核心与 PyO3 绑定放在独立 crate 中，由 maturin 构建 Python 包，以维持核心不依赖 Python 的边界。Python 包沿用 `kiyosi` 名称，按 CPython 版本构建普通 wheel；交付范围不扩展到自由线程 Python 或 ABI3 wheel。

迁移分阶段实现，完整验收后统一切换：迁移期间保留 C++ 实现用于对照，沿用现有数值算法，逐项完成 Rust 功能及其测试，并以独立正确性检查补充旧版对照。交付版本仅运行 Rust 核心，验收后删除旧 C++、CUDA 实现及其绑定和相关构建配置，不提供运行时双实现切换。
