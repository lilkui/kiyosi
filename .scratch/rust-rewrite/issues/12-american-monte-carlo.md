# 12: 美式蒙特卡洛

**What to build:** 用户可以用现有美式回归蒙特卡洛方法定价，并使用受支持的采样、Greeks 和隐含波动率能力。

**Blocked by:** 06: 美式近似定价；11: 欧式 Sobol 与对偶变量。

**Status:** completed

- [x] 沿用现有回归、行权策略、时间步网格与支持范围，保留美式最少步数等产品特定验证。
- [x] 接入既有伪随机、Sobol 与对偶组合，保留路径数、seed、线程配置及适用统计结果。
- [x] 迁移美式共享样本 Greeks、产品特定默认差分设置及反求语义，求解容差不被表述为采样误差保证。
- [x] 以现有美式 MC 回归、独立参考、统计与收敛检查验证，覆盖到期、稀少路径和回归边界。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## Comments

2026-10-28：本票已完成。

- 既有 `MonteCarloVanillaEngine` 增加 Rust `PricingEngine<AmericanOption>` 与 Python 美式分派，保留均匀时间网格、按执行价缩放的二次回归及线性/常数降阶、价内样本选择、逐期贴现与即时行权下界。到期前至少三点的验证位于适用性与合约生命期检查之后；到期仍接受两点。
- 复用伪随机 PCG32、Sobol 扰乱与对偶变量，Sobol 行权网格使用 `step_count - 1` 维；保留精确奇数路径、seed、逐调用线程限制和线程数无关的可复现性。路径生成使用既有分块调度，回归按确定顺序串行计算。
- Greeks 与隐含波动率反求在请求内固定 seed，重放相同正态样本，每次重新拟合行权策略；无持久状态。Python 默认或 `None` 现货差分使用 Rust 定义的现货 8%，其他默认值不变。无正股息、非负利率的看涨保留终端分布捷径；跨零利率 rho 两侧统一采用行权网格。
- 保留 `price_with_statistics` 的欧式支持范围；美式回归现金流共享拟合策略，不作为独立样本报告标准误差。用法文档明确采样、差分偏差与求解容差的区别，类型声明与独立 Rust 消费者同步。
- Rust 与 Python 公共 API 覆盖即时行权、到期、最少步数、稀少与秩亏回归、币值缩放、所有采样组合、奇数路径、线程一致性、共享样本、反求恢复、并发和失败状态。独立 QuantLib 比较保留 `american.json` 原有全部价格场景、wrapper Greeks 选择与精度预算，参考网格/差分稳定性检查不变；补充多 seed 的路径 RMSE、网格与默认 Greeks 收敛检查。
- Linux 验证：`PYO3_PYTHON=/nonexistent/python cargo +1.85.0 test --locked` 通过 97 项核心公共 API 测试和 26 个 rustdoc 示例。rustfmt、workspace Clippy（`-D warnings`）、Ruff、Python 类型检查、严格 rustdoc、Cargo 独立打包与 Rust 消费者均通过。
- `/tmp/kiyosi-issue12-candidate` 使用 CPython 3.15.1、QuantLib 1.43 隔离验收。源码 wheel、sdist 及从 sdist 重建 wheel 成功；安装重建 wheel 后完整候选 Python 套件 138 项全部通过。sdist 包含美式核心模块、更新后的类型声明与第三方声明。
- Standards 审查无发现；Spec 审查发现的一处过期文档已修复并复核，剩余发现为零。旧实现保留，未运行旧 C++/Python 套件，未新增性能门槛或报告。未验证 Windows 与其他 CPython 版本，未切换或发布正式交付。
