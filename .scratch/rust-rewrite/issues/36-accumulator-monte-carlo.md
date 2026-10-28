# 36: 累购蒙特卡洛

**What to build:** 用户可以用交易日 CPU 模拟定价累购，并获得现有统计结果、共享样本分析及隐含波动率能力。

**Blocked by:** 11: 欧式 Sobol 与对偶变量；34: 累购与数值积分。

**Status:** completed

- [x] 沿用累购交易日步进、累计支付、历史和终止规则，事件顺序与现有领域语义一致。
- [x] 接入现有伪随机、Sobol、对偶与线程配置，保留维度、路径数、seed 和适用统计结果。
- [x] 交付现有共享样本 Greeks、数值敏感度与隐含波动率行为，覆盖缺失历史、固定支付和不可识别状态。
- [x] 迁移累购 MC、对偶及线程一致性案例，通过积分或 FD 参考和统计检查验证，不要求 C++ 随机流逐位一致。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## Comments

### 完成核验（2026-10-28）

- 新增 Rust/Python `MonteCarloAccumulatorEngine`，复用累购核心状态及共享 CPU 采样器，按未来交易日进行 Actual/365 Fixed 精确对数正态转移；当前午夜事件只执行一次，达到 knock-out 先结算且不购买，严格低于 strike 才加速，累计单位在敲出或到期统一结算。
- 历史累计数量默认为零，零表示已知无累计；不新增未知历史哨兵或推断限制。保留伪随机、Sobol、对偶、精确路径数、完整维度、seed、线程配置及适用标准误与置信区间；固定 seed 的价格和统计结果跨线程数一致。
- 全部十项 Greeks 和 Rust 标量数值敏感度复用共享数值差分；Greeks 与隐含波动率求解固定请求内样本。保留事件边界、到期缺失 Greeks、固定支付与波动率不可识别状态、错误类别及失败后输入不变保证。
- Python 复用严格设置转换与 Rust 默认值，定价、统计、Greeks 和求解在拥有的核心输入上释放 GIL；同步公共导出、注册、求解分派、类型声明、rustdoc 与候选使用文档。Standards 与 Spec 两轴审查均无未解决问题。

| 要求 | 验收证据 |
| --- | --- |
| 交易日、累计支付、历史及终止顺序 | Rust `current_events_settle_before_sampling_with_strict_acceleration`、`trading_midnights_intraday_history_and_calendar_control_purchases`；Python `test_current_events_settle_before_sampling`、`test_midnight_intraday_calendar_and_omitted_history` |
| 伪随机、Sobol、对偶、维度及线程 | Rust `counts_pair_uncertainty_sobol_and_threads_preserve_the_same_samples`、`antithetic_pair_uses_both_opposite_normals_and_exact_path_counts`、`sobol_dimension_limit_uses_future_trading_dates_after_known_settlements`；Python `test_counts_sobol_antithetic_and_shared_engine_threads`、`test_sobol_dimension_limit_preserves_known_settlements` |
| 共享样本 Greeks、敏感度、反求及不可识别性 | Rust `shared_sample_greeks_and_volatility_solves_preserve_inputs`、`event_greeks_and_fixed_payoff_volatility_identifiability_follow_core_rules`；Python `test_shared_greeks_scalar_bumps_and_implied_volatility`、`test_rate_volatility_and_mixed_greeks_match_seeded_scalar_repricing` |
| 独立数值和统计参考 | Rust `simulated_cashflows_match_quadrature_with_sampling_error`；Python `test_terminal_and_multidate_prices_match_independent_references`，使用独立终端支付组合、积分价格及标准误容差 |
| 公共 API、默认值、严格转换及失败状态 | Rust `validation_precedes_settlement_and_failures_preserve_reuse`；Python `test_strict_settings_protocols_defaults_and_ownership`、`test_validation_fixed_payoffs_and_failure_reuse`、`test_installed_version_and_typed_exports` |
| 隔离候选、既有切片及交付范围 | 最终 `cargo test --release`：389 项通过（含 74 项 doctest）；隔离 release wheel 的 Python 全套：439 项通过。本票最终 Rust/Python 各 9 项测试通过；sdist 在全新构建目录重建 wheel 并在新环境通过本票及包检查 |

- Windows / CPython 3.15.1：rustfmt、Clippy（拒绝警告）、Ruff、ty、rustdoc、wheel/sdist 构建与重建、禁止 Python 发现的核心 Cargo 打包及独立 Rust 消费者检查均通过。调试构建下的本票 Rust/Python 测试也通过。
- 按仓库要求未运行旧 C++/Python 套件；未验证 Linux、其他 CPython 版本或最低 Rust 1.85，未增加性能门槛或报告。Monte Carlo Greeks 仍受样本量和差分步长影响；保留旧实现，不切换交付、不发布。
