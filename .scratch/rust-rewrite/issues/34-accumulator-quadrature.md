# 34: 累购与数值积分

**What to build:** 用户可以构造累购条款与累计历史，进行拥有值替换，并通过积分引擎获得价格和现有分析结果。

**Blocked by:** 03: 交易日历、观察日程与生成工具；08: 香草与数字期权数值积分。

**Status:** completed

- [x] 迁移累购条款、交易日事件、累计历史与到期支付，保留已有观察和支持范围。
- [x] 历史缺失、记录与估值日期不一致等状态在既有时机由核心验证，替换失败保持原产品和输入有效。
- [x] 使用现有累购积分方法，交付既有价格、Greeks、敏感度和适用的隐含参数支持，不新增算法。
- [x] 迁移累购历史与积分回归，以终端支付组合、独立可构造参考和收敛检查验证。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## Comments

### 完成核验（2026-10-28）

- 新增 Rust `AccumulatorTerms`、`Accumulator` 与 `QuadratureAccumulatorEngine`，保留交易日午夜观察、有效日起始事件、严格低于行权价时加速、达到敲出水平先结算且不购买、到期观察后支付的原语义；支持带符号价格、零购买量、零/单位加速和敲出水平不高于行权价。
- 历史保留原有调用方断言：累计数量默认为 Rust 定义的零，零是已知值，允许小数、历史更正和历史重估；原 API 没有未知历史哨兵或记录日期，不新增数量与估值日期的推断约束。构造/替换验证数量，定价依次验证平坦利率适用性、存续期和交易日到期；失败保留原产品和输入。
- 复用已有三次高斯积分、累计数量仿射双层、精确终端现金/资产矩与低价仿射尾部；单个未来观察不依赖空间预算。全部十项 Greeks 与隐含波动率复用核心数值分析、观察边界、请求内曲线复用、求解器及不可识别性契约，不新增算法或依赖。
- Python 仅处理严格协议、数值/日期转换、异常、所有权与 GIL；显式 `None`、布尔和错误数值表示保留原生异常，默认值来自 Rust。同步导出、类型声明、rustdoc、中英文 README、候选使用文档和独立 Rust 消费者。
- Standards 审查无问题；Spec 审查发现低价尾部先乘数量再贴现导致极端有限价格溢出的迁移回归，已恢复原计算顺序并在两种语言补充大数量回归。最终两轴均无未解决问题。

| 要求 | 验收证据 |
| --- | --- |
| 条款、历史拥有值与失败状态 | Rust `history_replacement_owns_validated_values_and_preserves_terms`、`validation_categories_and_order_preserve_accepted_degenerate_terms`；Python `test_contract_history_protocols_validation_and_ownership` |
| 当前观察、支付和错误时机 | Rust `settlement_and_current_purchase_preserve_exact_thresholds_and_timing`；Python `test_current_observation_adjacent_thresholds_and_extreme_result`、`test_settlement_boundaries_validation_and_failure_reuse`、`test_applicability_order_and_unidentifiable_volatility` |
| 独立终端支付与两事件参考 | Rust `single_observation_matches_terminal_digital_portfolio_without_grid_error`、`two_observations_match_independent_conditional_terminal_portfolio`；Python `test_single_observation_matches_independent_cash_asset_decomposition`、`test_two_events_match_independent_conditional_terminal_integral` |
| 原积分与 Greeks 精度标准 | Rust `spatial_and_domain_convergence_preserve_legacy_acceptance_targets`、`greeks_converge_under_spatial_refinement_and_smaller_bumps`；Python `test_representative_contracts_converge_in_space_and_domain`、`test_greek_availability_and_normalized_convergence` |
| 分析、反求、并发和曲线复用 | Rust `greeks_match_terminal_reference_and_cached_prices_match_scalar_repricing`、`analytic_availability_and_implied_volatility_follow_shared_contract`、`affine_history_zero_limits_and_tiny_volatility_tails`；Python `test_implied_volatility_uses_shared_solver`、`test_reused_spot_and_volatility_curves_match_scalar_prices`、`test_quantity_affinity_exact_limits_and_shared_instance_threads` |
| 极端贴现数量回归 | Rust `discounted_low_tail_preserves_large_finite_quantities`；Python `test_discounted_low_tail_preserves_large_finite_quantities` |

- Windows / CPython 3.15.1 核验通过：最终 `cargo test --release` 全套 **368 项（含 71 项 doctest）**；最终 sdist 在全新 Cargo 构建目录重建 release wheel，隔离候选环境安装后 `python -m unittest discover -s rust/python/tests -p 'test_*.py'` 全套 **417 项**通过。Rustfmt、Clippy（拒绝警告）、Ruff、ty、严格 rustdoc、禁用 Python 发现的核心 Cargo 打包与独立 Rust 消费者检查通过，文档示例运行通过。
- 保留旧实现，按仓库要求未运行旧 C++/Python 套件；累购 FD/MC 及其跨引擎回归留待后续票实现，当前以独立终端组合、两事件积分和原有收敛标准验收。未验证 Linux、其他 CPython 版本或最低 Rust 1.85；未新增性能门槛、性能报告、发布或切换交付。
