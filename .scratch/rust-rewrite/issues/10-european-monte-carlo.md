# 10: 欧式蒙特卡洛与 CPU 并发

**What to build:** 用户可以进行欧式 CPU 伪随机蒙特卡洛定价，获取适用的统计与分析结果，并按调用控制线程数。

**Blocked by:** 04: Greeks、数值敏感度与隐含波动率。

**Status:** completed

- [x] 保留欧式精确终端分布采样、路径数、步数验证、可选 seed 和相关核心默认值，不引入中间步进改变算法。
- [x] 交付适用的标准误差、置信区间、共享样本 Greeks 和隐含波动率行为；一次分析或求解按现有规则共享 seed。
- [x] 线程数的自动、串行与指定上限按调用生效，不修改进程全局并发设置；同一构建内固定 seed 的结果保留既有线程数无关性。
- [x] Python 核心计算按线程安全契约释放 GIL，释放期间不触碰 Python 对象；同步、信号和资源失败语义有文档。
- [x] 通过解析参考、统计正确性和公开 API 并发案例验收，验证路径与线程边界，不比较线程加速比。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## Comments

2026-10-28：核对欧式 CPU 蒙特卡洛实现，本票已完成。

- `rust/core/src/monte_carlo.rs` 实现精确终端采样、核心默认值与配置验证、标准误差和置信区间；数值 Greeks 与隐含波动率复用核心分析接口，每次请求固定一个 seed，失败保留输入。
- 固定采样块及归并顺序保证同一构建内跨线程数一致性；线程上限按调用生效，无全局可变并发设置。Python 绑定捕获拥有的 Rust 输入并释放 GIL；导出、类型声明和 `docs/rust-candidate.md` 已同步，文档覆盖同步、信号、资源失败和并发契约。
- Rust `rust/core/tests/monte_carlo.rs` 的 8 项与 Python `rust/python/tests/test_monte_carlo.py` 的 11 项测试覆盖解析及实时 QuantLib 参考、统计覆盖率、独立标量 Greeks、seed 共享、线程和路径边界、并发、GIL、表示错误、验证时机及失败状态。
- 本次 Linux 验证：`PYO3_PYTHON=/nonexistent/python cargo test --locked` 通过全部 85 项核心公共 API 测试及 25 个 rustdoc 示例；`cargo fmt --all --check`、workspace Clippy（`-D warnings`）、核心文档构建、Cargo 打包验证及独立 Rust 消费者通过。
- 使用 maturin 从当前源码构建 wheel，安装到 `/tmp/kiyosi-issue10-candidate` 独立环境；CPython 3.15.1、QuantLib 1.43 下完整候选套件 `python -m unittest discover -s rust/python/tests -p 'test_*.py' -v` 的 122 项测试全部通过。
- 旧实现保留；按仓库要求未运行旧 C++、旧 Python 套件，未比较线程加速比。未验证 Windows、其他 CPython 版本、最低 Rust 1.85 或 sdist 重建；未切换或发布正式交付。
