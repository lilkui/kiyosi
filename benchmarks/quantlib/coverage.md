# 覆盖范围与排除项

默认清单包含 59 个场景，涉及 20 个符合条件的引擎，覆盖 51 个必测语义单元。[suite_coverage.py](suite_coverage.py) 独立于 `cases/` 维护必测的引擎/模型/产品/行权方式/赔付类型/激活方式/结算时点清单。报告中的 `coverage.cells` 记录匹配的用例 ID，以及两个原生可执行程序在 `--info` 注册表中的支持情况。

未指定 `--case` 时，即使传入替代清单，也要求覆盖全部必测单元；缺失单元会生成 `missing_coverage` 行。重复传入 `--case ID` 会进入聚焦运行模式（`coverage.mode = focused`），此时仍审计所提供的清单，但不要求包含未选中的单元。匹配基于适配器的实际输入与隐含合约语义，而非用例名称；常数局部波动率曲面不能满足非常数曲面单元的要求。

未知 ID 或不支持的语义会生成 `unsupported` 行；构建、版本或动态链接加载失败会生成 `prerequisite_failed` 行。缺失参考值或后端响应格式错误也会使运行失败。所有未通过行的比值均为 `null`。

## 排除的引擎

以下引擎在相同合约、模型与算法下，没有经过验证的 QuantLib 内置直接对应实现。仅价格相近、代理组合复制或自定义 QuantLib 产品引擎均不符合纳入条件。

| 引擎 | 排除原因 |
| --- | --- |
| BjerksundStenslandVanillaEngine | Kiyosi 使用 2002 版近似公式；QuantLib 使用 1993 版。 |
| AnalyticRangeAccrualEngine | QuantLib 的利率区间累计在合约与模型语义上不同。 |
| QuadratureBarrierEngine | 未找到匹配的内置数值积分引擎。 |
| QuadratureAccumulatorEngine, QuadratureAutocallableEngine | 未找到原生的直接合约与引擎实现。 |
| CoxRossRubinsteinDigitalEngine | QuantLib 的二叉树香草引擎要求普通香草收益。 |
| FiniteDifferenceDoubleBarrierEngine, FiniteDifferenceDoubleTouchEngine | 未验证到匹配的原生 BSM/局部波动率有限差分引擎。 |
| FiniteDifferenceAccumulatorEngine, FiniteDifferenceAutocallableEngine | 未找到原生的直接合约与引擎实现。 |
| MonteCarloDigitalEngine | QuantLib 的美式首达数字期权不同于欧式到期数字期权。 |
| MonteCarloAsianEngine | 连续平均不同于 QuantLib 的离散定盘蒙特卡洛。 |
| MonteCarloAccumulatorEngine, MonteCarloAutocallableEngine | 未找到原生的直接合约与引擎实现。 |

## 已匹配引擎的限制

- 美式数值积分：QuantLib 的 QdFpAmerican 通过迭代求解行权边界定价，而非逆向积分延续价值。
- 有限差分二元障碍：在 BSM 与给定局部波动率下，已匹配到期结算的现金/资产二元障碍、到期结算的单不触碰以及触碰即付的单触碰；到期结算的单触碰尚无经过验证的通用原生映射。
- 解析双障碍：仅限连续监控 BSM、零返还金且行权价严格位于上下障碍之间。双触碰仅匹配到期现金赔付；排除 KIKO/KOKI 混合触发与资产赔付双触碰。
- 蒙特卡洛障碍：仅限零返还金的连续监控单障碍。
- 亚式期权：仅限自估值日至到期日的新起算（unseasoned）连续平均、平坦利率且无历史样本。算术平均用例排除接近零的持有成本与奇异 Levy 分母；详见[测试方法](methodology.md#数字期权与亚式期权)。
- 障碍与触碰合约：仅限初始未触碰的连续监控合约，不含定期监控、历史记录或离散分红跳变。解析香草障碍的敲入返还金在到期支付，敲出返还金在触碰时支付。触碰即付的不触碰期权在业务上无实际合约意义。

本测试套件并未覆盖所有收益类型、方向、数值格式、随机数生成器或虚实值程度组合，也不包含显式的希腊值请求。针对具体用例的精度门限不能证明所有业务输入下的精度，也不构成普适的性能排名。
