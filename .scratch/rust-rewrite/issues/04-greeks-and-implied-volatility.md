# 04: Greeks、数值敏感度与隐含波动率

**What to build:** 用户可以对欧式期权请求现有 Greeks 和数值敏感度，并从价格反求隐含波动率，获得可辨别的成功结果或失败类别。

**Blocked by:** 02: 期限结构与市场替换。

**Status:** completed

- [x] 迁移既有 Greek 请求、解析与数值结果、差分设置和结果可用性规则；覆盖混合请求、默认值、估值与到期边界。
- [x] 公开隐含波动率接口保留区间、收敛控制、价格校验、无括区间、不收敛、非有限求解和参数不可识别时的错误语义。
- [x] 在 Rust 核心提供供后续引擎复用的既有分析与求解行为，Python 不复制领域规则或默认值。
- [x] 通过公开 API 的 Greeks 对照、差分步长稳定性、波动率恢复和实时 QuantLib 案例验证，不以误差预算放宽掩盖差异。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## 实现与验证（2026-10-27）

- 核心新增 `Greek`、`GreeksRequest`、拥有值 `PricingResult`、统一差分与求解默认值；解析欧式引擎提供十项 Greeks。`EuropeanPricingEngine`、`numerical_greeks` 和 safeguarded Brent `implied_volatility` 供后续欧式引擎复用，Python 仅适配协议、转换、异常与拥有值。到期不可识别、无括区间、不收敛、非有限求解、原生定价失败分类保持可辨别；解析价格和 Greeks 共用稳定的 BSM 参数计算。
- 混合请求、可用性及验证时机：Rust `selections_expiry_and_validation_timing_preserve_availability`、`numerical_stencils_keep_feasible_measures_and_fail_on_feasible_pricing_errors`；Python `test_requests_availability_and_expiry`、`test_domain_errors_validation_order_and_failure_state`。
- 求解边界与错误：Rust `solver_categories_bounds_and_failure_state_are_stable`、`non_finite_solver_values_and_pricing_errors_remain_distinct`；Python `test_solver_endpoints_curves_and_concurrent_calls`、`test_strict_shift_and_solver_representations`。校验整数范围、拒绝 bool/浮点到整数等有损转换，失败保留原市场对象。
- 数值依据：Python `test_live_quantlib_greeks_scalar_differences_and_recovery` 覆盖既有 20 个欧式实时 QuantLib 1.43 场景，在两个 spot 步长下核对标量价格差分并恢复波动率，沿用原误差预算和独立 oracle uncertainty 检查。Rust `numerical_greeks_are_stable_across_absolute_bumps`、`curve_knots_use_right_hand_time_sensitivities_and_parallel_rho`、`scaled_tail_greeks_and_selected_extreme_results_remain_representable` 覆盖差分稳定性、曲线节点右导数及高精度尾部回归。
- Windows Rust 1.99.1 通过全部 27 个核心公共 API 测试及 2 个 rustdoc 示例；CPython 3.15.1 候选版通过全部 45 个测试，旧版隔离环境通过全部 44 个共享测试（欧式 8、市场 13、日历 16、分析 7）。旧版完整异形产品套件未重跑，旧实现未修改或删除。
- rustfmt、Clippy（拒绝警告）、Ruff、Python 类型检查通过。核心在无效 Python 探测路径下通过测试、文档、Cargo 打包和独立下游消费。wheel 安装及最终 sdist 在新临时目录重建 wheel 后的隔离安装测试均通过；4 个 Rust 和 4 个 Python 文档示例通过执行检查。导出、类型声明、用法与并发/GIL 契约同步。
- 未验证 Linux、其他 CPython 版本及最低 Rust 1.85；未切换正式交付、未发布包，未新增性能门槛。

## Standards

发现一个类型声明与运行时不一致问题和一个非阻断重复代码问题，均已修复：`PricingResult` 沿用既有不可直接构造的 `Never` 声明，解析价格与 Greeks 共用稳定 BSM 参数计算。独立复核最终修改无新增问题；剩余 0 项。

## Spec

发现并修复一个数值 speed 可用性回归：仅请求 speed 且 ±2h 不可行时，不应执行 ±h 定价。最终 guard 保留有限基础价格并返回不可用 speed，真实极端解析案例与回归测试均通过。独立复核未发现缺失需求、范围扩张或其他错误行为；剩余 0 项。
