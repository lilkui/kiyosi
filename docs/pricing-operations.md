# 定价操作支持表

先按合约与模型选择引擎，再按要改变的输入选择操作。标量估值使用 `price(instrument, context)`，需要敏感度时使用 `price_with_greeks(instrument, context, greeks=...)`。引擎文档说明其敏感度估计方式和适用条件。

| 操作 | 引擎 | 支持的合约和模型 |
| --- | --- | --- |
| `price_many` | `CoxRossRubinsteinVanillaEngine` | 欧式或美式香草合约，可混合传入。平坦利率 BSM。 |
| `price_many` | `MonteCarloAutocallableEngine` | 一批同类雪球、二元雪球、三元雪球或 Phoenix 合约。平坦利率 BSM。 |
| `price_strikes` | `QuadratureVanillaEngine`、`MonteCarloVanillaEngine` | 美式香草模板。平坦利率 BSM。 |
| `price_strikes` | `MonteCarloAsianEngine` | 算术平均亚式模板。BSM。 |
| `price_strikes` | `AnalyticHestonEuropeanEngine` | 欧式香草模板。Heston。 |
| `price_strikes`、`price_surface`、`solve` | `FiniteDifferenceVanillaEngine` | 欧式或美式香草。BSM、局部波动率。 |
| `price_strikes`、`price_surface`、`solve` | `FiniteDifferenceHestonVanillaEngine` | 欧式或美式香草。Heston。 |
| `solve` | `FiniteDifferenceDigitalEngine` | 现金或资产数字期权。BSM、局部波动率。 |
| `solve` | `FiniteDifferenceBarrierEngine`、`FiniteDifferenceDoubleBarrierEngine` | 对应的单障碍、双障碍期权。BSM、局部波动率。 |
| `solve` | `FiniteDifferenceBinaryBarrierEngine`、`FiniteDifferenceDoubleTouchEngine` | 二元障碍与单触碰合约，或双触碰合约。BSM、局部波动率。 |
| `solve` | `FiniteDifferenceAutocallableEngine`、`FiniteDifferenceAccumulatorEngine` | 对应的雪球系列与 Phoenix，或累购、累沽。BSM、局部波动率。 |
| `price_with_statistics` | `MonteCarloVanillaEngine` | 欧式香草。平坦利率 BSM。 |
| `price_with_statistics` | `MonteCarloDigitalEngine`、`MonteCarloBarrierEngine` | 对应的数字期权或单、双障碍期权。平坦利率 BSM。 |
| `price_with_statistics` | `MonteCarloAsianEngine` | 算术或几何平均亚式。BSM。 |
| `price_with_statistics` | `MonteCarloAutocallableEngine`、`MonteCarloAccumulatorEngine` | 对应的雪球系列与 Phoenix，或累购、累沽。平坦利率 BSM。 |

- 批量给已有合约估值时，传入合约序列。Autocallable Monte Carlo 的 `shared_grid=True` 可在兼容日程间共享路径，固定种子下的结果可能与逐笔价格不同。
- 只变行权价时使用 `price_strikes`。同时变更行权价和到期日时使用 `price_surface`，结果按输入的到期日排列，每行按输入的行权价排列。
- 只变现价时，对有限差分引擎调用一次 `solve`。在求解时指定的现价区间内，使用 `solution.price(spot)` 或 `solution.price_many(spots)` 查询，定价网格无需重新回滚。敏感度以 `solution.supported` 和每次查询的状态为准。[示例](../examples/python/finite_difference_batch.py)
- 需要采样不确定性时使用 `price_with_statistics`。只需记录本次采样种子时使用 `price_with_seed`。美式香草 Monte Carlo 支持记录种子和使用独立路径估计行权收益的 `price_american_out_of_sample`。

Rust 从 `kiyosi::PricingEngine` 调用标量定价、敏感度和采样统计方法。从 `kiyosi::engines` 导入 `FiniteDifferenceEngine` 以调用 `solve`，或导入 `VanillaBatchEngine` 以调用有限差分的行权价批量和价格曲面方法。

引擎方法文档列出监控方式、时间约定、曲线覆盖、历史状态和工作量限制。障碍和触碰合约的 `observation_schedule` 可直接用于另一个合约的构造或替换，同时携带观察时点和日历。Rust 使用 `option.observation_schedule().cloned()` 获得可传入条款的日程。复用模型利率时，读取 `risk_free_curve` 或 `dividend_curve` 以保留曲线覆盖。[观察日程示例](../examples/python/barrier_option.py) · [曲线示例](../examples/python/rate_curves.py)

Python 用 `implied_coupon` 求报价，再用 `note.with_coupon(**quote)` 将报价应用到新合约。雪球求解时需指定 `quote_convention`，结果包含首个观察日的年化票息与到期票息联动约定。Phoenix 报价是每次满足派息条件时支付的票息率。Rust 使用 `implied_snowball_coupon` 和 `implied_phoenix_coupon`。[雪球示例](../examples/python/snowball.py) · [Phoenix 示例](../examples/python/phoenix.py)
