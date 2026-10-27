# 01: 欧式解析定价贯通

**What to build:** Rust 和 Python 用户可以在隔离安装的 Rust 候选包中构造欧式期权及 BSM 市场上下文并获得解析价格，同时保留旧实现用于迁移对照。

**Blocked by:** None (can start immediately)。

**Status:** completed

- [x] 建立独立 Rust 核心与 PyO3 绑定 crate，由 maturin 构建候选 Python 包；核心能在无 Python 和绑定框架参与时构建、测试并由独立 Rust 消费方调用。
- [x] 欧式看涨、看跌的有效输入、估值时点和到期边界沿用现有领域语义；通过公开 API 得到价格，并与现有解析案例及独立参考对照。
- [x] 引入本切片所需的核心错误类别、拥有值、严格数值与日期转换和从核心派生的 Python 默认值；文档区分领域拒绝、表示错误、资源耗尽及内部失败。
- [x] 候选包具有一致的版本、导出和类型声明；建立最小构建与公共 API 测试检查，记录同步调用和线程安全默认契约。
- [x] 旧版与候选版在独立环境或进程中验证，旧实现与既有交付保持可用；不增加用户可见的双实现切换开关。

## 实现与验证（2026-10-27）

- 使用说明与检查命令见 [Rust candidate](../../../docs/rust-candidate.md)；核心位于 `rust/core`，候选 Python 包构建入口为 `rust/python/pyproject.toml`，根目录继续交付旧版。
- Linux CPython 3.11、3.12、3.13、3.14、3.15 的普通 wheel 均构建、隔离安装并通过全部 9 个候选 API/打包测试，其中复用 20 个现有欧式场景与实时 QuantLib 1.43 对照。
- 独立旧版环境通过共享的 8 个欧式契约测试；既有 Python 全套 487 个测试通过，5 个后端条件测试按既有环境规则跳过；其中 2 个 CPU 构建的后端不可用检查随后在 `KIYOSI_TEST_CUDA=disabled` 下单独通过，其余 3 个需要 GPU。
- Rust 核心通过 6 个公共 API 测试及 1 个 rustdoc 示例；Rust 1.85 最低版本的核心测试与绑定检查通过。核心独立构建、文档、打包和工作区外消费均在 Python 探测路径无效时通过。
- rustfmt、Clippy（拒绝警告）、Ruff、Python 类型检查通过。候选 sdist 可在临时目录重建 wheel，安装后通过候选测试；许可证和类型声明随包交付。
- 并行 Standards / Spec review 均为 0 项实质问题。Windows 尚未执行验证；本切片未发布包，也未切换正式交付。
