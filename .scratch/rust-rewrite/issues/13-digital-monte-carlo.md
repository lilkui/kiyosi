# 13: 数字期权蒙特卡洛

**What to build:** 用户可以对现金或无和资产或无数字期权使用现有 CPU 蒙特卡洛支持组合。

**Blocked by:** 05: 数字期权解析定价；11: 欧式 Sobol 与对偶变量。

**Status:** completed

- [x] 保留数字支付、结算和终端采样语义，支持现有伪随机、Sobol、对偶变量与线程配置。
- [x] 交付适用的价格、统计结果、Greeks、数值敏感度和反求能力，不增加既有支持矩阵之外的操作。
- [x] 覆盖不连续支付、到期、平值以及既有大额支付、折现和溢出回归，结构化错误保持一致。
- [x] 通过数字解析参考、现有统计案例、固定 seed 及跨线程数检查验证两种语言的等价场景。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。


## Comments

2026-10-28：本票已完成。

- 新增 Rust `MonteCarloDigitalEngine` 与 Python 同名引擎，支持现金或无和资产或无的看涨/看跌数字期权。复用既有 CPU 终端采样、PCG32、Sobol 扰乱、对偶变量、精确路径计数、逐调用线程上限和确定顺序归约；步数校验但不参与终端采样，领域默认值仍只定义于 Rust。
- 保留标准化坐标的严格行权测试、到期结算及既有似然比 delta/gamma；其他引擎 Greeks 保持不可用，数值差分设置仍先校验。复用 Rust `numerical_greeks` 与共享隐含波动率求解器，每个分析/求解请求固定 seed，失败不改变引擎或输入。按既有数字 MC 支持矩阵，不新增 `price_with_statistics`。
- 支付与似然权重采用适用的对数缩放，共用矩累计支持带符号 Greeks 并避免求和或平方溢出。保留支付样本/所请求 Greek 样本非有限时的 `InvalidResult`，未请求 Greek 的溢出不影响标量价格。节点曲线在合约生命期、请求及求解参数校验之前返回 `UnsupportedOperation`。
- Python 仅适配严格的表示转换、异常、拥有值和 GIL 释放；导出、类型声明、求解器分派、独立 Rust 消费者及用法文档同步。所有四种采样组合、到期/平值、微小波动率、大额支付、折现、错误顺序、奇数路径、固定 seed、线程一致性、并发和失败后恢复通过公共 API 验证。
- 迁移既有数字 CPU MC 价格与似然 Greeks 的数值预算、负利率/股息及微小波动率回归；独立比较全部 40 个现有实时 QuantLib 数字场景，并在两种语言中验证多 seed 现金支付 RMSE 与独立 Bernoulli 方差。现金数字求解保留既有恢复预算；资产数字使用局部恢复预算，文档说明样本支付跳跃与多根限制。
- Linux 验证：`PYO3_PYTHON=/nonexistent/python cargo +1.85.0 test --locked` 通过全部 103 项核心公共 API 测试和 27 个 rustdoc 示例。rustfmt、workspace Clippy（`-D warnings`）、Ruff、Python 类型检查、严格 rustdoc、Cargo 独立打包及独立 Rust 消费者均通过。
- `/tmp/kiyosi-issue13-candidate` 使用 CPython 3.15.1、QuantLib 1.43 隔离验收。源码 wheel、sdist、从解包 sdist 重建的 debug/release wheel 均成功；安装从 sdist 重建的 release wheel 后完整候选 Python 套件 146 项通过。Standards 与 Spec 两项独立审查均无实质性发现。
- 旧实现保留，未运行旧 C++/Python 套件，未新增性能门槛或报告。未验证 Windows 与其他 CPython 版本，未切换或发布正式交付。
