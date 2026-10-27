# 07: 香草与数字期权 CRR

**What to build:** 用户可以通过两种公开 API 对现有香草与数字期权支持组合使用 CRR 二叉树，并请求适用的分析结果。

**Blocked by:** 05: 数字期权解析定价；06: 美式近似定价。

**Status:** completed

- [x] 覆盖现有欧式、美式香草及现金或无、资产或无数字期权的 CRR 支持矩阵，不扩大类型或领域支持范围。
- [x] 步数与相关配置默认值来自核心，保留参数边界、风险中性概率及数值结果有效性的验证与错误。
- [x] 保留美式提前行权、数字支付与结算边界，以及各组合既有 Greeks、数值敏感度和隐含波动率能力。
- [x] 在公共 API 验证网格细化、定价与 Greeks 回归，并与现有适用的独立参考或解析极限比较。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## Comments

2026-10-28：Rust 核心与 Python 候选导出 `CoxRossRubinsteinVanillaEngine` 和
`CoxRossRubinsteinDigitalEngine`，核心 `BinomialSettings` 定义默认 256 步及
1–1,000,000 边界。保留既有 CRR 算法、平坦利率限制、风险中性概率与树节点
数值检查、美式提前行权和严格数字支付边界。香草补全数值 Greeks，数字仅返回
节点 delta/gamma；反求括区间在核心与树适用域相交。适配层仅做协议、类型分派、
异常与拥有值转换，反求释放 GIL；类型声明、候选文档及双语 README 已同步。

| 要求 | 公共 API 验证依据 |
| --- | --- |
| 支持矩阵与节点价格、Greeks | Rust `european_tree_refines_to_analytic_price_and_node_greeks`、`digital_tree_matches_one_step_cash_and_asset_payments`；Python `test_node_greeks_and_prices_match_explicit_rollback` |
| 默认值、表示边界、验证时机和错误 | Rust `settings_flat_curve_validation_and_unstable_trees_preserve_inputs`；Python `test_configuration_representation_and_domain_boundaries`、`test_flat_rate_applicability_validation_order_and_failures` |
| 提前行权、欧式极限与数字到期边界 | Rust `american_early_exercise_and_no_dividend_limit`、`expiry_boundaries_settle_strictly_and_life_errors_precede_trees`；Python `test_american_immediate_exercise_and_european_limit`、`test_expiry_strict_equality_and_lifetime_boundaries` |
| 数值敏感度、可用性与失败状态 | Rust `vanilla_numerical_completion_tracks_analytic_and_propagates_bump_failures`、`one_step_speed_omits_an_infeasible_scalar_stencil`；Python `test_digital_greek_availability_and_unused_shifts`、`test_one_step_speed_omits_infeasible_scalar_stencil` |
| 反求与树适用域 | Rust `implied_brackets_intersect_tree_domain_for_all_supported_contracts`；Python `test_implied_volatility_tree_bounds_failures_and_preserved_state` |
| 收敛、负 carry、极端尾部与独立参考 | Rust `digital_odd_even_and_off_strike_grids_refine_with_negative_carry`、`deterministic_endpoints_and_underflowed_tail_weights_remain_finite`；Python `test_grid_refinement_and_negative_carry`、`test_deterministic_endpoints_and_extreme_tail_regression`、`test_existing_quantlib_european_and_american_profiles`，复用原 QuantLib 1.43 profiles、稳定性与误差预算 |
| 拥有值、并发、导出与分发 | Python `test_shared_and_distinct_instances_support_concurrent_pricing`、`test_installed_version_and_typed_exports`；独立 `tests/rust-consumer`、Cargo 包及 sdist 重建 wheel |

- Windows 核心 `cargo test`：56 个公共 API 测试和 16 个 rustdoc 示例通过。
- 核心测试、文档、Cargo 包验证及独立下游消费者在无效 `PYO3_PYTHON` 下通过。
- CPython 3.15.1 完整隔离候选测试：80 项通过，运行于从最终 sdist 重建并安装的 wheel。
- 独立 legacy 环境：共享 CRR 12 项通过；审查新增的一步 speed 回归单独通过。
- rustfmt、Clippy `-D warnings`、Ruff、ty、wheel、sdist 及用法示例检查通过。
- 未运行旧 C++ 测试（旧行为未修改）；未验证 Linux、其他 CPython 版本或最低 Rust 1.85；未发布或切换正式交付。

## Standards

独立审查发现一步香草树缺少 native gamma 时，speed 错误使用不可行的 ±2h
标量 stencil。已在共享数值 helper 检查该 stencil 的正值、有限值及对称性；
不可行时 speed 不可用，native gamma 仍只需 ±h。Rust/Python 红灯复现后通过，
既有 native gamma 单 bump 回归保持通过。复审剩余 0 项。

## Spec

独立审查剩余 0 项；支持矩阵、默认值、验证顺序、支付边界、Greeks 可用性、
反求范围及绑定/类型/文档符合本票。旧实现保留，无新增依赖、性能门槛或报告。
