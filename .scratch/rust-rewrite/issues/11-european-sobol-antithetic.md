# 11: 欧式 Sobol 与对偶变量

**What to build:** 用户可以在欧式蒙特卡洛中选择现有 Sobol 或伪随机采样及对偶变量，并获得符合采样方法的结果。

**Blocked by:** 10: 欧式蒙特卡洛与 CPU 并发。

**Status:** completed

- [x] 迁移现有 Sobol 扰乱、数字位移、seed 和维度规则，方向数数据及其第三方许可完整保留。
- [x] 保留 Sobol 路径数约束及不报告标准误差和置信区间的规则，不把准蒙特卡洛结果伪装成伪随机统计量。
- [x] 对偶变量支持范围与路径计数保持现状，覆盖允许的奇数路径、不成对路径及与 Sobol 组合的边界。
- [x] 所有现有欧式采样组合通过价格、分析操作、可复现性及跨线程数案例；允许随机流不同于 C++，不要求逐位跨实现比较。
- [x] 从混合计时工具中保留统计与精度断言，不把采样速度优势或耗时比较作为通过条件。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。


## Comments

2026-10-28：本票已完成。

- Rust 核心增加 `MonteCarloSampling::{PseudoRandom, Sobol}` 与 `antithetic` 配置，保持默认伪随机采样、精确终端分布和已有路径、步数、seed、线程规则。Python 仅处理严格的字符串/布尔表示转换、异常、拥有值及 GIL 释放；导出、类型声明、独立 Rust 消费者和用法文档已同步。
- 移植既有 52 位 LMS 扰乱、数字位移、Gray-code 索引和 Cephes 逆正态变换。欧式使用一维，Sobol 总路径数必须为 1..10,000,000 内的二次幂；对偶变量使用 ceil(path_count/2) 个基样本，伪随机奇数路径保留最后一个不成对路径。
- 方向数表完整保留，逐项核对全部 397,013 个整数与旧数据一致。Cargo 包与 Python sdist 包含数据，核心包、wheel 和 sdist 保留 Joe–Kuo 与 Cephes/SciPy 第三方许可。
- Sobol 始终不报告标准误差或置信区间，包括到期。偶数伪随机对偶路径以独立配对均值估计方差，至少需要两对；到期前的奇数对偶路径不报告不确定性。验证对偶伴随路径失败时错误传播、输入不变及后续请求可恢复。
- Rust 蒙特卡洛文件 13 项、Python 蒙特卡洛文件 18 项测试覆盖四种采样组合、既有十项 Greeks 精度预算、独立标量差分、解析与 QuantLib 参考、多 seed 的统计覆盖率和 RMSE、奇数路径与采样块边界、线程无关性、分析与求解共享 seed、并发、表示边界及失败状态。修正安装包导出检查，使其按既有 Literal 别名规则识别 `MonteCarloSampling`。
- Linux 验证：`PYO3_PYTHON=/nonexistent/python cargo +1.85.0 test --locked` 通过全部 90 项核心公共 API 测试及 25 个 rustdoc 示例；rustfmt、workspace Clippy（`-D warnings`）、Ruff、Python 类型检查、核心文档构建、Cargo 独立打包验证和独立 Rust 消费者通过。
- 在 `/tmp/kiyosi-issue11-candidate` 隔离环境用 CPython 3.15.1、QuantLib 1.43 验证；maturin 源码 wheel、sdist 和从解包 sdist 重建 wheel 均成功，安装重建 wheel 后完整候选 Python 套件 129 项全部通过。
- 独立 Standards 与 Spec 审查均无实质性发现。旧实现保留；按仓库要求未运行旧 C++ 或旧 Python 套件，未增加速度比较或性能门槛。未验证 Windows、其他 CPython 版本，未切换或发布正式交付。
