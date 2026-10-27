# 05: 数字期权解析定价

**What to build:** 用户可以构造现金或无、资产或无数字期权，并通过 Rust 与 Python 获取现有解析价格及支持的分析结果。

**Blocked by:** 04: Greeks、数值敏感度与隐含波动率。

**Status:** completed

- [x] 迁移两类数字期权及现有方向、支付与结算语义，保留构造和替换验证、拥有值及默认值。
- [x] 交付既有解析价格、Greeks、数值敏感度和可适用的隐含波动率能力；不扩展不受支持的产品与操作组合。
- [x] 覆盖价内、价外、平值、到期、估值边界和既有溢出、折现下溢回归；领域输入和结果错误保持既有类别。
- [x] 迁移相关数字期权公开 API 测试，通过现有独立定价及 Greeks 参考验证结果。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## 实现与验证（2026-10-27）

Rust 核心以拥有值 `DigitalOption::cash_or_nothing` / `asset_or_nothing` 表达两种支付，`AnalyticDigitalEngine` 复用既有稳定 BSM 参数及对数加权概率；delta/gamma 使用解析公式，其余 Greeks 使用共享差分完成。`EuropeanContract` 只暴露合约存续日期，`EuropeanPricingEngine<I>` 使共用 `numerical_greeks` 与 safeguarded Brent 求解支持数字期权，同时由类型限定引擎/产品组合。默认差分和求解参数仍只定义于 Rust。数字价格可能对波动率非单调，求解保留调用者括区间，不新增搜索或算法。

Python 保留 `CashOrNothingOption`、`AssetOrNothingOption` 及 `kiyosi.pricing.AnalyticDigitalEngine` 的主要调用方式。绑定只适配严格协议转换、异常、所有权和 GIL；数字解析价格/Greeks 保留 GIL，反求在拥有的 Rust 数据上释放 GIL。构造没有隐式支付默认值。数字合约替换是候选 API：旧实现没有 `.replace()`；候选的替换通过核心构造验证，失败保留原值，不能改变支付种类。类型声明、公开导出、[使用文档](../../../docs/rust-candidate.md#digital-contracts-and-analytic-pricing) 与独立下游消费示例同步。

| 要求 | 公共 API 验证依据 |
| --- | --- |
| 构造、拥有值、替换与失败状态 | Rust `constructors_and_replacements_validate_before_returning_owned_values`；Python `test_constructor_domains_and_native_representation_errors`、候选专用 `test_replacements_validate_and_preserve_originals` |
| 价格、解析/数值 Greeks、请求选择与反求 | Rust `cash_and_asset_prices_match_independent_quantlib_values`、`analytic_delta_gamma_and_completed_greeks_preserve_selections`；Python `test_prices_greeks_and_volatility_recovery`、`test_requests_settings_solver_errors_and_failure_state` |
| 价内/价外/平值及独立参考 | Rust `itm_and_otm_native_greeks_match_quantlib`；Python `test_live_quantlib_prices_greeks_and_scalar_differences` 复用既有全部 40 个数字 QuantLib 1.43 场景，沿用原精度预算和独立 uncertainty 校验，平滑场景另以两个 spot 步长核对标量价格差分；一天到期场景按既有套件核对价格与原生 delta/gamma |
| 到期严格执行价、估值边界、时间差分 | Rust `expiry_is_strict_and_valuation_life_is_inclusive_to_the_microsecond`、`expiry_reaching_time_stencils_use_paired_scalar_fallback`；Python `test_strict_expiry_payoffs_and_valuation_boundaries`、`test_expiry_reaching_time_stencils_use_scalar_derivatives_on_both_sides` |
| 溢出、下溢、极小波动率与尾部结果 | Rust `log_scaled_prices_survive_discount_and_settlement_overflow_and_underflow`、`selected_greeks_preserve_finite_results_and_tiny_volatility_gamma`、`asset_delta_retains_independent_high_precision_tail_values`；Python `test_scaled_discounts_and_overflowing_settlements`、`test_selected_overflow_tiny_volatility_and_scaled_tail_greeks` |
| 原生 speed、差分可用性与曲线契约 | Rust `speed_uses_native_gamma_at_large_price_scales`、`native_gamma_speed_needs_only_a_single_positive_spot_bump`、`digital_analysis_preserves_settings_solver_and_curve_horizon_failures`；Python `test_native_speed_large_scales_and_single_bump_availability`、`test_curves_coverage_time_knots_and_concurrency` |
| 语言边界与包装 | Python `test_constructor_domains_and_native_representation_errors`、`test_requests_settings_solver_errors_and_failure_state` 拒绝 bool、无效日期/选择及整数窄化；`test_installed_version_and_typed_exports` 校验安装版本、导出与类型成员；独立下游 `tests/rust-consumer` 覆盖数字价格、Greeks 与反求 |

- Windows 上 `cargo test` 通过全部 39 个 Rust 公共 API 测试及 9 个 rustdoc 示例；设置无效 `PYO3_PYTHON` 后核心测试、文档、Cargo 打包及独立下游消费仍通过。
- CPython 3.15.1 候选环境运行 `python -m unittest discover -s rust/python/tests -p 'test_*.py'`，全部 56 个测试通过。旧版隔离环境运行共享欧式 8、市场 13、日历 16、分析 7、数字 10 个测试，全部 54 个通过。
- `cargo fmt --all --check`、workspace Clippy（拒绝警告）、Ruff 检查/格式和 `ty check rust/python/python` 通过。Cargo 包独立验证、候选 wheel 安装、sdist 在干净环境中重建并安装验证通过；全部 5 个 Rust 和 5 个 Python 用法示例通过执行检查。
- 旧 C++/Python 实现保留且未改动；未引入依赖、产品/引擎组合、性能门槛或性能报告。未验证 Linux、其他 CPython 版本、最低 Rust 1.85；未重跑旧版全部异形产品套件，未切换正式交付或发布。

## Standards

独立审查发现 gamma 分支重复计算权重与符号，已合并。最终复核无文档标准违规或剩余问题。

## Spec

独立审查发现并修复到期时间差分回退问题：当任一时间端点没有原生 delta/gamma 时，两端必须同时使用标量差分，不能混用原生与标量值。新增 Rust/Python 回归先失败后通过，共享数字套件在新旧实现均通过；最终复核剩余 0 项。审查另以 6,000 个极端尺度场景比较价格、delta、gamma 及错误类别，18,000 项对照未发现其他差异。
