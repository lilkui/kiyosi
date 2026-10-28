# 35: 累购有限差分

**What to build:** 用户可以用 FD 定价累购产品并请求现有 Greeks 与隐含波动率，保留交易日和历史边界。

**Blocked by:** 09: 香草与数字期权有限差分；34: 累购与数值积分。

**Status:** completed

- [x] 复用已验证的 FD 配置与累购领域条款，迁移既有累购状态、观察事件、累计支付和到期处理。
- [x] 保留估值时点、未知历史、已实现累计与未来事件的验证和替换失败保证。
- [x] 交付现有 Greeks、数值敏感度及隐含波动率能力，维持求解设置、参数可识别性和错误类别。
- [x] 迁移累购 FD 精度、混合 Greeks、插值和事件回归，以积分参考、终端支付及网格收敛核对。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。

## Comments

### 完成核验（2026-10-28）

- 新增 Rust/Python `FiniteDifferenceAccumulatorEngine`，复用已有 FD 配置、空间/时间网格、曲线、步进、事件投影和样条插值；迁移累计数量仿射双层、未来交易日事件、到期支付、事件后 Rannacher 阻尼重启与资产/现金分别贴现的上边界。
- 复用累购核心状态和历史语义：累计数量默认为零，零是已知历史，允许小数、更正和历史重估；原 API 没有未知历史哨兵或记录日期，不新增推断限制。午夜当前事件只执行一次；严格低于 strike 才加速，达到 knock-out 先结算且不购买。存续期、交易日到期验证先于立即结算；只有未来现金流要求完整曲线和有效网格。
- 支持平坦/节点曲线、全部十项 Greeks、Rust 标量数值敏感度与共享隐含波动率求解；保留单位、默认 bump、选择性请求、事件边界、不可识别性、错误类别和输入不变保证。Python 默认值来自 Rust，仅负责严格转换、异常、所有权与 GIL，定价/分析/求解在拥有的核心值上释放 GIL。
- 同步 Rust 导出、Python 注册/定价/求解分派、类型声明、中英文 README、候选使用文档及独立 Rust 消费者；保留旧实现，不新增依赖、算法、性能门槛或报告。
- Standards 与 Spec 两轴审查均发现 native speed 缺失旧版的移位价格溢出检查；已恢复两端价格检查，并在 Rust/Python 添加极大累计数量的失败类别与原值保留回归。复核后两轴均无未解决问题。

| 要求 | 验收证据 |
| --- | --- |
| FD 配置、累计状态、事件和到期 | Rust `settlement_thresholds_history_replacement_and_expiry_greeks_are_exact`、`trading_midnight_is_observed_once_and_intraday_uses_caller_history`；Python `test_midnight_intraday_and_calendar_events_are_applied_once`、`test_settlement_history_boundaries_and_failure_reuse` |
| 估值、历史验证及替换失败保证 | Rust `validation_order_domains_and_failures_preserve_engine_and_inputs`；Python `test_settlement_history_boundaries_and_failure_reuse`、`test_validation_precedes_state_and_bumped_failures_leave_inputs_intact`；节点曲线由两端 `node_curves_price_fixed_holdings_and_only_future_cashflows_require_coverage` / `test_node_curves_terminal_forward_and_coverage_order` 验收 |
| Greeks、敏感度、反求与不可识别性 | Rust `spline_and_mixed_greeks_match_independent_public_repricing`、`implied_volatility_recovers_quotes_and_preserves_solver_failures_and_identifiability`；Python `test_native_interpolation_mixed_greeks_and_event_availability`、`test_implied_solver_identifiability_protocols_and_failure_state`、`test_market_greeks_and_implied_volatility_preserve_independent_results` |
| FD 精度、插值、混合 Greeks 与收敛 | Rust `terminal_event_converges_to_independent_digital_portfolio`、`daily_layers_converge_to_quadrature_and_quantity_is_affine`；Python `test_terminal_averaging_converges_against_analytic_prices_and_greeks`、`test_daily_event_averaging_and_damping_stabilize_prices_and_greeks`、`test_quantlib_terminal_portfolios_and_separate_grid_convergence`（18 个独立 QuantLib 组合，空间/时间/域分别加密） |
| 两端公共 API、默认值、导出、类型与文档 | Python `test_settings_protocols_defaults_exports_and_ownership`、`test_installed_version_and_typed_exports`；独立 Rust 消费者、ty、严格 rustdoc、73 项 doctest、两段累购使用示例均通过 |
| 非法输入、稳定错误与失败状态 | Rust `speed_propagates_nonfinite_shifted_prices_without_changing_inputs`；Python `test_speed_propagates_nonfinite_shifted_prices_and_preserves_inputs`、`test_settings_protocols_defaults_exports_and_ownership`；同时覆盖布尔、浮点转整数、溢出、错误协议和引擎/产品不匹配 |
| 隔离候选及已完成切片、交付范围 | `cargo test`：306 项集成测试及 73 项 doctest，合计 379 项全部通过；最终隔离 Python 环境 `python -m unittest discover -s rust/python/tests -p 'test_*.py'`：430 项全部通过；最终 sdist 从全新构建目录重建 wheel，另一个隔离环境安装后 23 项累购契约、3 项 FD 精度及 1 项包/类型检查全部通过 |

- Windows / CPython 3.15.1：rustfmt、Clippy（拒绝警告）、Ruff、ty、严格 rustdoc、wheel/sdist 构建及重建、禁止 Python 发现的核心 Cargo 打包与独立 Rust 消费者均通过。最终 sdist 核验包含审查后恢复的 speed 价格检查。
- 按仓库要求未运行旧 C++/Python 套件；未验证 Linux、其他 CPython 版本或最低 Rust 1.85。累购 Monte Carlo 留待票 36，不切换交付、不发布。
