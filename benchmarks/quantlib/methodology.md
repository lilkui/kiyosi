# 基准测试方法

误差预算、数值参数配置与蒙特卡洛验证门限均在 `cases/*.json` 中显式声明。参考值来源与收敛验证结果位于 `references/*.json` 和 `calibration/*.json`；实测误差与计时结果见这些产物文件，本文档不另行列出。常规运行直接使用已提交的参数配置与参考值，不执行参数调优或参考值生成。

两个适配器均采用 Actual/365 Fixed 日计数与连续复利利率。两端的数值方法、网格区域、区间/节点数及随机数流可以不同，但须先通过相同的精度门限，再对比延迟。

## 数字期权与亚式期权

QuantLib 对到期结算的现金/资产数字期权使用 `AnalyticEuropeanEngine`，对连续几何平均亚式期权使用 `AnalyticContinuousGeometricAveragePriceAsianEngine`，对连续算术平均亚式期权使用 `ContinuousArithmeticAsianLevyEngine`。其 Turnbull–Wakeman 引擎采用离散平均。

平均期自估值日起算，不含历史记录。几何平均合约使用新起算（unseasoned）构造函数；若显式传入起算日，则会进入不支持的已起算（seasoned）路径。算术平均合约传入起算日，并将当前累计均值设为零。算术平均用例要求 `abs(r-q)`、`abs(r-q+sigma²)` 和 `abs(2(r-q)+sigma²)` 均不小于 `1e-6`。该对比校验双方共用的双矩对数正态近似，而非相对真实算术平均亚式期权价格的误差。独立 Decimal 公式在 50/80 位有效数字下做精度收敛核验；误差预算为 `1e-9`，参考值不确定度为 `1e-12`。

## 障碍期权与触碰期权

| 合约 | QuantLib 原生工具/引擎 | 结算方式 |
| --- | --- | --- |
| 单障碍香草期权 | `BarrierOption` / `AnalyticBarrierEngine` | 敲出返还金在触碰时支付；敲入返还金在到期支付 |
| 到期现金/资产二元障碍期权 | `BarrierOption` / `AnalyticBinaryBarrierEngine` | `AmericanExercise(..., payoffAtExpiry=true)` 保留到期行权价条件 |
| 单触碰期权 | `VanillaOption` / `AnalyticDigitalAmericanEngine` | 收益行权价等于障碍价；由行权标志选择触碰即付或到期支付 |
| 单不触碰期权 | `VanillaOption` / `AnalyticDigitalAmericanKOEngine` | 未触碰存续时在到期支付 |
| 双障碍香草期权 | `DoubleBarrierOption` / `AnalyticDoubleBarrierEngine` | 零返还金；行权价位于上下障碍之间 |
| 双障碍现金触碰/不触碰期权 | `DoubleBarrierOption` / `AnalyticDoubleBarrierBinaryEngine` | 欧式行权；到期支付 |

连续监控下触碰即付的资产价值等于障碍水平，因此可用支付该金额的原生现金二元期权表示。到期二元障碍期权还会保留到期行权价条件，有别于无条件触碰期权。

[generate_barrier_references.py](generate_barrier_references.py) 将吸收壁布朗运动的对数价格密度与现金、资产和香草收益函数积分；触碰即付部分采用首达时间的拉普拉斯变换。Decimal 计算采用 90/110 位有效数字，双障碍每侧取 8/12 个镜像项，正态尾部截断设为 16 个标准差。收敛验证证据支撑各场景 `1e-12` 的参考值不确定度与 `1e-9` 的解析误差预算。QuantLib 双障碍引擎的级数项数在参数配置中显式指定；其双障碍二元引擎固定使用 99 项 Hui 级数，没有公开的收敛控制参数，因此采用独立价格门限。

## 有限差分

BSM 与给定局部波动率用例涵盖欧式/美式香草期权、现金/资产数字期权、单障碍香草期权与到期二元障碍期权、到期结算单不触碰期权以及触碰即付单触碰期权。数字期权直接使用原生数字收益类型。有限差分触碰期权将收益行权价设为障碍水平：

| 合约 | 原生障碍类型 | 二元期权方向 | 返还金 |
| --- | --- | --- | --- |
| 向下不触碰 | DownOut | Call | 0 |
| 向上不触碰 | UpOut | Put | 0 |
| 向下单触碰（触碰即付） | DownOut | Put | 现金赔付额，或资产赔付对应的障碍价 |
| 向上单触碰（触碰即付） | UpOut | Call | 现金赔付额，或资产赔付对应的障碍价 |

反向二元收益在未触碰存续时支付为零，触碰即付金额由返还金提供。两个适配器均拒绝已触碰的障碍和不支持的结算方式。Kiyosi 一维有限差分以对数现货为坐标。普通欧式用例使用 `upper=automatic`（`asset_upper_boundary=None`）。其余一维用例显式指定现货上界，下界自动选择，连续障碍固定相应端点。QuantLib 使用自动对数网格，阻尼与敏感度计算的差异保留在计时中。

普通欧式和连续单障碍普通期权在行权价折点沿对数现货做终端收益单元平均，包含局部波动率敲入的香草延续价值。
BSM 普通欧式自动网格以扣除确定性持有成本后的行权价加密节点，回推完成后再应用股息贴现。

`calibration/fd_bsm.json` 与 `calibration/fd_local_vol.json` 记录 `selected` 主配置，主配置使用自动域时另记录 `explicit_boundary` 显式上界对照。`domain_constant_spacing` 扩大上界，并以整数区间数保持主网格近似相同的平均 `Δln S`。例如下障碍 80 的 `[80,400]`、800 区间扩展为 `[80,600]`、1002 区间。上障碍固定主网格上界时，仅在扩域会改变延续价值网格的情况下记录扩域对照。合约价位附近加密的节点间距取决于求解域。

局部波动率的 Kiyosi 主配置使用 800 个空间步和 100 个时间步。美式看跌和向上资产不触碰使用 200 个时间步，以保留误差余量。`initial_time_400` 保留 400 个时间步的对照。局部波动率敲入另有 `continuation_constant_spacing` 探针，用于控制全域延续价值网格的平均对数步长。两个网格共用区间数，因此扩域会同时影响另一网格的空间误差。

固定网格规模的计时不代表达到任意误差预算的最低耗时；不同网格策略的相同节点数也不代表相同局部分辨率。

`compare_fd_accuracy.py` 分别验证调用策略、显式网格核心算法及主配置的影响。主配置与显式对照相同时，只测量显式网格在两个构建之间的差异。比较采用用例误差预算与参考不确定度，要求欧式自动网格调用及 BSM/局部波动率欧式看涨、下敲出看涨的显式网格误差下降，延迟增幅不超过 5%。运行方式见 [README](README.md)，默认报告 `calibration/fd_accuracy.json` 记录构建、采样协议与原始样本。

BSM 用例的误差预算为 `0.003`。欧式参考值复用独立 Decimal 公式；美式看跌期权共用 [calibration/american.json](calibration/american.json) 中 Rust 与 QuantLib 在时间、空间和求解域上的收敛结果，经验不确定度为 `5e-5`。QuantLib 的向上资产不触碰与向上现金单触碰需要更多时间步来消除终端边界误差，仅加密空间网格无法修复该误差。精确参数配置以及独立的空间、时间和求解域检查记录在 `cases/fd_bsm.json` 和 `calibration/fd_bsm.json` 中。

局部波动率用例的误差预算为 `0.005`。给定的波动率曲面在时间与现货维度上做线性插值，对现货端点做截断，并要求日期在覆盖范围内。QuantLib 将按时间行组织的输入转置后填入 `FixedLocalVolSurface`，并启用 `localVol=true`。其 `mesher_vol` 仅控制自动网格尺寸；微分算子的波动率不来自隐含波动率或 Dupire 转换。

局部波动率参考值通过独立的 QuantLib 收敛驱动程序生成，分别调整时间步数、空间网格和网格器求解域。资产数字期权与资产到期二元障碍期权需要格外细密的空间网格；后者使用隐式欧拉格式生成参考值，以避免终端跳变振荡。不确定度取观测到的时间、空间与求解域偏移量之和的两倍再加 `1e-10`（经验估计），上限为用例误差预算的 10%。未通过的收敛方案保留在 `calibration/local_vol_rejected_references.json` 中；资产数字与资产二元障碍的检查还对 Rust 的网格与求解域做了独立收敛验证。

## Heston

用例涵盖解析欧式看涨期权，以及有限差分欧式看涨期权和美式看跌期权，误差预算分别为 `1e-7` 和 `0.005`。这些常规平值（ATM）用例并非尾部或稳定性普查。Kiyosi 的自适应傅里叶积分使用确定性方差控制变量；QuantLib 使用 Gatheral 表示配合自适应 Lobatto 积分。欧式参考值由独立的 Lobatto 与 exp-sinh 积分收敛计算提供。

Kiyosi 有限差分采用改进 Craig–Sneyd ADI 格式与 Ikonen–Toivanen 美式分裂。标的轴使用对数现货，方差轴包含零方差。基准显式设置现货和方差上界。QuantLib 采用 Hundsdorfer 格式、原生美式收益投影与自动求解域。校准分别扩大现货与方差域，现货扩域保持近似相同的平均对数步长。美式参考值结合了联合网格与各维度独立外推证据，其经验不确定度不超过用例误差预算的 10%。

## 蒙特卡洛

两个后端均使用伪随机抽样，关闭对偶路径与控制变量。随机种子为非零 32 位整数；两个后端以相同种子生成各自的路径。验证使用 32 个预设种子 `1009 + 97*i`（`i=0..31`），计时使用独立种子 `1729`。种子集在计时前运行（含 `--price-only` 模式），门限依据这组种子的经验结果。每次新调用都会重新生成样本。

欧式看涨、下敲出看涨和美式看跌各有一对工作线程配置。基线的两个后端各使用一个工作线程，`-8-workers` 用例把 Kiyosi 的工作线程数设为 8，沿用基线的路径数、种子、参考价格及精度门限。QuantLib 1.44 内置引擎逐条生成并处理路径，在两组配置中均使用一个工作线程。[QuantLib 1.44 的路径采样循环](https://github.com/lballabio/QuantLib/blob/v1.44/ql/methods/montecarlo/montecarlomodel.hpp#L82-L113)

Kiyosi 的基准适配器在定价前检查可用 CPU 并设置专用线程池的上限。首次并行预热创建线程池，计时测量复用该线程池的定价延迟。工作线程配置沿用单工作线程用例的路径数校准，两个配置均独立运行精度校验。

欧式与障碍用例要求绝对误差加参考值不确定度不超过 `0.10`、RMSE 不超过 `0.05`、有符号偏差绝对值不超过 `0.025`、样本标准差不超过 `0.05`。参考值复用独立的 Decimal BSM 公式与吸收密度公式。路径数与时间步数的收敛结果与逐种子结果一并记录在 `calibration/monte_carlo.json` 中。

Kiyosi 使用 PCG32/Box–Muller 与条件桥存续概率；QuantLib 使用 MT/逆正态抽样，并用固定种子为 5 的独立 MT 随机流抽样障碍穿越。两端的障碍协议均拒绝主种子 5，以避免随机流耦合。`calibration/monte_carlo_seed_collision.json` 记录使用种子 1 至 32 验证随机流耦合的证据。验证种子在检查验证结果前确定。路径数均指物理路径数。

### 美式最小二乘蒙特卡洛

美式看跌期权使用共用的细化有限差分参考值。门限要求绝对误差加不确定度不超过 `0.15`，RMSE 与样本标准差不超过 `0.075`，有符号偏差绝对值不超过 `0.05`。

Kiyosi 使用基函数 `1, S/K, (S/K)^2` 拟合实值定价路径，并带低阶回退。QuantLib 使用独立的校准路径、二次单项式加内置收益基函数，并将校准种子偏移 10000。两者在每次定价请求时均重新拟合。QuantLib 的校准、方差与行权概率计算保留在计时范围内。路径数、行权网格与校准路径数的收敛结果记录在 `calibration/american_monte_carlo.json` 中；仅靠路径标准误无法约束回归与行权偏差。

## 维护

执行原生参考值生成或校准前，先用 `run.py` 构建配套程序。应先确定误差预算与验证种子，再选择后端参数配置。场景变更后，先重新生成相应参考值、校准参数配置，再运行精度门限与测试，最后解读计时结果。脚本会覆盖已提交的产物文件，请审阅生成的差异。收敛计算可能耗时数分钟。

| 用例文件 | 参考值生成 | 校准证据 |
| --- | --- | --- |
| `european.json` | `generate_european_references.py` | 参考值文件内含 Decimal 精度收敛结果 |
| `digitals_asian.json` | `generate_digitals_asian_references.py` | 参考值文件内含精度证据；原生检查见 `test_digitals_asian.py` |
| `barriers.json` | `generate_barrier_references.py` | `calibrate_barriers.py` |
| `numerical.json` | `calibrate_numerical.py` | 同一脚本；同时刷新共用的 `calibration/american.json` |
| `fd_bsm.json` | `generate_fd_references.py` | `calibrate_fd.py` |
| `fd_local_vol.json` | `generate_local_vol_references.py` | `calibrate_local_vol.py` |
| `heston.json` | `generate_heston_references.py` | `calibrate_heston.py` 写入 `heston_calibration.json` |
| `monte_carlo.json` | `generate_mc_references.py` | `calibrate_monte_carlo.py` |
| `american_monte_carlo.json` | `calibrate_american_monte_carlo.py`，复用共用的美式有限差分证据 | 同一脚本 |

请在仓库根目录下运行脚本，例如 `python3 benchmarks/quantlib/generate_fd_references.py`。刷新依赖美式有限差分结果的 FD/CRR/LSM 参考值前，需先刷新共用的美式有限差分证据。Python 美式 Heston 测试复用 `references/heston.json`；Heston 生成脚本的增量探针输出为 `build/heston-refinements.jsonl`。部分离线驱动程序会直接调用 `quantlib-config`，因此除配置运行时链接器外，还需将目标安装路径加入 `PATH`。
