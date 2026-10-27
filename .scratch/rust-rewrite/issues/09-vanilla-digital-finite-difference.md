# 09: 香草与数字期权有限差分

**What to build:** 用户可以用现有有限差分方案定价支持的香草、数字期权，并处理美式行权及相应分析请求。

**Blocked by:** 05: 数字期权解析定价；06: 美式近似定价。

**Status:** completed

- [x] 迁移现有 FD 方案、网格、阻尼及相关配置规则，支持矩阵与核心默认值保持一致。
- [x] 覆盖欧式终端支付、美式提前行权、数字不连续支付及其既有边界处理，不切换数值方法。
- [x] 交付既有 Greeks、数值敏感度和隐含波动率能力；验证混合 Greeks、步长、网格及配置错误。
- [x] 迁移现有 FD 网格细化、Rannacher、插值、折现和 Greeks 数值回归，通过公共 API 与独立参考验证。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。


## Comments

2026-10-28：Rust 核心及隔离 Python 候选交付 `FiniteDifferenceVanillaEngine`、
`FiniteDifferenceDigitalEngine` 与共享配置。保留显式 Euler、隐式 Euler、
Crank–Nicolson、两段 Rannacher 隐式半步、Thomas 求解、单边漂移稳定化及样条插值。
默认 800 个空间步、200 个时间步；欧式与数字采用均匀网格，美式使用随波动率、
期限及累计漂移扩展的 sinh 网格。香草支持节点曲线并对齐利率节点；数字保持显式
平坦利率限制、终端单元平均及折现支付界限。提前行权与边界逐半步更新。

原生样条 delta/gamma、同网格 speed、混合与其他数值 Greeks 复用核心数值补全，
所有扰动共享中央空间域。隐含波动率复用现有标量求解器，保留调用者区间及
显式方案稳定性拒绝。Python 只处理表示、协议、拥有值、异常及调用，默认值来自
Rust；价格、分析与反求在拥有的核心输入上释放 GIL。导出、类型声明、双语 README、
候选使用说明及独立 Rust 消费者已同步。无新增依赖、算法或性能门槛，旧实现保留。

| 要求 | 公共 API 验证依据 |
| --- | --- |
| 支持矩阵、方案、默认值、配置验证 | Rust `settings_validate_without_allocating_or_imposing_grid_caps`、`schemes_track_prices_and_stability_includes_drift`；Python `test_settings_defaults_representation_validation_and_immutability` |
| 欧式、提前行权、数字支付与边界 | Rust `american_exercise_and_continuation_boundaries_survive_negative_rates`、`node_curves_align_time_steps_and_keep_optimal_intermediate_exercise`、`scaled_digital_discounts_and_strict_expiry_payments_remain_representable`；Python `test_curve_support_and_validation_order`、`test_expiry_life_and_clipped_grid_validation` |
| Greeks、数值敏感度与反求 | Rust `spline_greeks_use_one_mesh_and_selective_mixed_requests`、`implied_volatility_reuses_the_scalar_engine_for_all_contracts`；Python `test_mixed_greeks_track_analytic_sensitivities`、`test_one_sided_native_time_greeks_remain_selective_and_accurate`、`test_implied_volatility_and_solver_failures`；独立消费者验证 `numerical_greeks` |
| 网格细化、低波动、Rannacher、插值与折现 | Rust `terminal_prices_converge_to_independent_references`、`rannacher_startup_equals_implicit_half_steps_for_all_contracts`、`low_volatility_drift_stabilization_converges_and_large_grids_are_supported`、`interpolation_and_implicit_euler_preserve_large_currency_scales`、`payoff_bounds_allow_carry_error_but_reject_oscillations`；Python 迁移 `test_fd_rannacher.py`、`test_fd_mixed_greeks.py`、`test_digital_finite_difference_discounts.py` |
| 独立参考与近到期、高波动长期域 | Python `test_existing_quantlib_european_and_american_profiles`、`test_existing_quantlib_digital_profiles`、`test_refined_one_day_vanilla_prices_and_native_greeks`、`test_american_long_expiry_domain_matches_live_quantlib`，复用原 QuantLib 1.43 场景、误差预算、稳定性、排除规则与数字边界细网格 |
| 错误类别、验证时机、失败状态及表示边界 | Rust `validation_timing_expiry_and_failures_preserve_inputs`；Python `test_analysis_requests_shifts_failures_and_input_ownership`、`test_expiry_life_and_clipped_grid_validation`；覆盖最大 i32 时间步数与裁剪空间域的立即拒绝 |
| 并发、导出、类型与分发 | Python `test_shared_and_distinct_engines_support_concurrent_analysis`、`test_installed_version_and_typed_exports`；rustdoc、独立 Rust 消费者、Cargo 包、sdist 重建 wheel 及使用示例 |

- Windows 完整核心 `cargo test`：77 项公共 API 测试及 22 个 rustdoc 示例通过。
- 核心测试、文档、Cargo 包验证及独立消费者在无效 `PYO3_PYTHON` 下通过。
- 最终 sdist 重建并安装的 wheel：CPython 3.15.1 隔离候选完整 111 项测试通过。
- rustfmt、Clippy `-D warnings`、Ruff、ty、分发与新增用法示例检查通过。
- 按仓库要求未运行旧 C++ 或旧 Python 套件；未检查 Linux、其他 CPython 版本及
  最低 Rust 1.85；未发布或切换正式交付。

## Standards

独立审查发现空间域验证之前分配时间网格，极大时间步数可能使应有的领域拒绝
变成内存耗尽。已先验证空间网格，并从曲线节点直接解析美式自动域，分析阶段
不再为冻结空间域分配时间网格；Rust 与 Python 最大 i32 配置回归通过。
复审：0 项剩余发现。

## Spec

独立审查同样指出上述验证时机问题。修正后支持矩阵、配置、边界、Greeks、
反求、绑定及文档符合本票，未发现范围扩展。复审：0 项剩余发现。
