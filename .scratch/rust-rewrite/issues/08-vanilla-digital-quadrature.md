# 08: 香草与数字期权数值积分

**What to build:** 用户可以在 Rust 与 Python 中使用现有数值积分引擎定价受支持的香草和数字期权。

**Blocked by:** 05: 数字期权解析定价。

**Status:** completed

- [x] 迁移现有积分方法与香草、数字支持组合，保留积分配置、截断与精度参数的默认值和验证。
- [x] 交付现有价格、适用的 Greeks、数值敏感度和隐含波动率调用，不为未支持组合增加隐式回退。
- [x] 覆盖到期、极端有效市场输入、数字支付和折现边界，并验证非法设置和无效数值结果的错误类别。
- [x] 迁移现有积分回归与收敛案例，通过解析或独立参考验证，供后续路径依赖积分产品复用已验证的行为。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。


## Comments

2026-10-28：Rust 核心与隔离 Python 候选导出 `QuadratureVanillaEngine` 和
`QuadratureDigitalEngine`，支持欧式香草、现金或无及资产或无数字期权。
保留既有固定 2,048 面板 Simpson 积分、相对端点密度 `exp(-72)` 截断、
补偿求和、尾部与折现的对数缩放，以及小波动宽度稳定化。
现有终端引擎没有公开积分配置；`QuadratureSettings` 仅用于后续路径依赖产品，
本票不新增配置。保持显式平坦利率限制、严格数字支付边界及全部标量数值 Greeks，
复用核心隐含波动率求解与验证；适配层只处理类型、协议、异常和拥有值转换。
价格、Greeks 与反求在拥有的 Rust 输入上释放 GIL；导出、类型声明、候选文档及双语
README 已同步。旧实现及原有 QuantLib 数据、误差预算保持不变，无新增依赖或性能门槛。

| 要求 | 公共 API 验证依据 |
| --- | --- |
| 支持矩阵与既有积分价格 | Rust `terminal_quadrature_matches_vanilla_cash_and_asset_references`；Python `test_terminal_prices_match_independent_references` |
| 高方差、负利率与小正波动率 | Rust `high_variance_negative_rates_and_small_widths_match_analytic_limits`；Python `test_high_variance_negative_carry_and_small_width_limits` |
| 极端尾部、数字支付、折现下溢 | Rust `scaled_tails_and_underflowed_discounts_preserve_representable_payments`；Python `test_scaled_tail_and_discount_regressions`，沿用独立 100 位 Decimal 参考与 erfc 参考 |
| 到期严格边界、生命周期与 Greeks 可用性 | Rust `expiry_is_strict_for_digitals_and_omits_all_greeks`；Python `test_strict_expiry_and_lifetime_boundaries` |
| 全部数值 Greeks、bump 收敛、拥有结果 | Rust `numerical_greeks_refine_and_implied_volatility_preserves_inputs`；Python `test_numerical_greeks_converge_and_results_own_their_values` |
| 平坦利率、非法配置、原生表示错误与失败状态 | Rust `applicability_validation_order_and_nonfinite_results_are_structured`、`feasible_bumped_failures_preserve_inputs_and_dated_flat_rates_are_supported`；Python `test_applicability_representation_validation_and_failure_state`、`test_feasible_bumped_failures_propagate_without_changing_inputs` |
| 反求与稳定错误类别 | Rust `numerical_greeks_refine_and_implied_volatility_preserves_inputs`；Python `test_implied_volatility_recovery_and_solver_failures` |
| 既有独立参考 | Python `test_existing_quantlib_vanilla_profiles`、`test_existing_quantlib_digital_profiles`，复用原 QuantLib 1.43 场景、稳定性与误差预算；沿用一天数字场景只验价格的既有 profile 边界 |
| 无限对数折现的有限零价格与补偿 Greeks | Rust `infinite_discount_limits_preserve_zero_prices_at_small_width`；Python `test_extreme_forwards_and_compensated_greeks` |
| 并发、导出、类型与分发 | Python `test_shared_and_distinct_engines_support_concurrent_pricing`、`test_installed_version_and_typed_exports`；独立 `tests/rust-consumer`、Cargo 包及 sdist 重建 wheel |

- Windows 完整核心 `cargo test`：64 个公共 API 测试及 18 个 rustdoc 示例通过。
- 核心测试、文档、Cargo 包验证及独立下游消费者在无效 `PYO3_PYTHON` 下通过。
- 最终 sdist 重建并安装的 wheel：CPython 3.15.1 隔离候选完整 92 项测试通过。
- rustfmt、Clippy `-D warnings`、Ruff、ty、wheel/sdist 和新增用法示例检查通过。
- 按仓库要求未运行旧 C++ 与旧 Python 套件；未验证 Linux、其他 CPython 版本或最低 Rust 1.85；未发布或切换正式交付。

## Standards

独立审查及修正后复审：0 项剩余发现。领域行为保留在 Rust，绑定使用原生转换、
拥有输入与无 GIL 核心计算；导出、类型声明、文档及公开 API 回归保持一致。

## Spec

独立审查发现小波动宽度稳定化调用解析 helper 时，helper 会拒绝无限对数折现，
导致旧平坦利率算法可表示的零价格变成 `InvalidResult`。已限制稳定化仅在两个
对数折现均有限时调用，其他情况沿用积分极限；Rust/Python 红灯复现后通过。
新增回归覆盖看涨/看跌、相等及不等极端正利率、普通/微小/次正规波动率。
复审：0 项剩余发现。
