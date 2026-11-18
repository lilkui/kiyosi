# QuantLib 原生基准测试

在两者通过相同精度门限的前提下，对比 Kiyosi Rust 与 QuantLib C++ 的延迟。支持的覆盖子集见[覆盖范围与排除项](coverage.md)，参考值与算法细节见[测试方法](methodology.md)。

## 环境要求

- Linux、Rust/Cargo、Python 3.12+（仅需标准库）。
- C++17 编译器与 Boost 头文件。
- 使用 `-O3 -DNDEBUG` 编译的 QuantLib C++ **1.44**，且可通过 `quantlib-config` 和运行时链接器找到。仅安装 Python 版 QuantLib 包无法满足要求。仅使用 Cargo 的库开发工作流不需要 QuantLib。

## 运行方式

在仓库根目录下执行：

```sh
# 构建两个配套程序，校验精度，对全部用例计时并写入 report.json
python3 benchmarks/quantlib/run.py

# 聚焦单个用例；重复指定 --case 可选择多个用例
python3 benchmarks/quantlib/run.py --case european-bsm-atm-call --report /tmp/bsm.json

# 使用已有二进制文件校验精度，不计时
python3 benchmarks/quantlib/run.py --no-build --price-only
```

若 QuantLib 安装在非标准路径：

```sh
QUANTLIB_CONFIG=/opt/quantlib-1.44/bin/quantlib-config \
LD_LIBRARY_PATH=/opt/quantlib-1.44/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} \
python3 benchmarks/quantlib/run.py
```

| 选项 | 用途 |
| --- | --- |
| `--case ID` | 选择指定用例，可重复传入。未指定该选项时，即使使用自定义清单，也必须包含全部必测覆盖单元。 |
| `--cases PATH`, `--references PATH` | 加载 JSON 文件或目录，默认为运行脚本同级的 `cases/` 和 `references/`。使用部分清单时需配合 `--case`。 |
| `--no-build` | 复用 `target/release/quantlib_bench` 和 `benchmarks/quantlib/build/quantlib_bench`。 |
| `--rust-executable PATH` | 与 `--no-build` 一起使用，指定已有的 Rust 适配器。默认构建模式使用 Cargo 报告的可执行文件路径。 |
| `--price-only` | 仅运行精度门限与蒙特卡洛种子集校验，不计时。 |
| `--target-ms MS` | 自适应计时批次的最短时长，默认为 `20`。 |
| `--report PATH` | JSON 报告输出路径，默认为 `benchmarks/quantlib/report.json`。 |
| `--quantlib-config`, `--cxx`, `--cxxflags` | 覆盖构建配置，也可通过 `QUANTLIB_CONFIG`、`CXX`、`CXXFLAGS` 设置。 |

运行 `python3 benchmarks/quantlib/run.py --help` 查看全部选项。

## 解读结果

两端的价格均须满足 `abs(price - reference) + reference_uncertainty <= error_budget`。蒙特卡洛用例还会跨验证种子检查经验误差、偏差与离散度。门限未通过时跳过计时；未通过行的比值为 `null`，并使命令以失败状态退出。单个用例失败后，其余已请求的用例仍会继续运行。

计时基于预先构造的合约、定价上下文、模型与引擎，交替采集 15 组双后端样本。每次请求都会重新计算最新价格；后端固有的额外开销（包括 QuantLib 的敏感度与样本方差计算）均保留在计时范围内。比值定义为 `Q/K`，大于 1 表示 Kiyosi 延迟更低。JSON 报告完整保留原始样本、后端参数配置、参考值证据、覆盖情况以及机器与构建元数据。请在空闲机器上运行测量；结果仅适用于所记录的场景与参数配置。

## 测试与维护

覆盖范围与报告检查无需安装 QuantLib：

```sh
python3 -m unittest discover -s benchmarks/quantlib -p test_coverage.py
```

运行完整测试套件前，需先构建两个配套程序。即使未设置 `KIYOSI_QUANTLIB_LIVE`，部分测试也依赖原生二进制文件；设置该变量会启用额外的 QuantLib 检查：

```sh
python3 benchmarks/quantlib/run.py --price-only --case european-bsm-atm-call
KIYOSI_QUANTLIB_LIVE=1 python3 -m unittest discover -s benchmarks/quantlib -p 'test_*.py'
```

原生适配器分别为 [quantlib.cpp](quantlib.cpp) 和 [quantlib_bench](rust/src/bin/quantlib_bench/main.rs)。[Rust benchmark 包](rust/Cargo.toml)不发布，包含适配器和离线校准程序 [quantlib_bench_references](rust/src/bin/quantlib_bench_references.rs)，两者只使用 Kiyosi 的公开 Rust API。

场景输入、误差预算与后端参数配置位于 `cases/*.json`；参考值、不确定度与来源信息位于 `references/*.json`；离线收敛证据位于 `calibration/*.json` 和 `heston_calibration.json`。修改场景时，请遵循[参考值与校准流程](methodology.md#维护)。

比较 FD 调用策略与核心精度时，基线程序须为 release 构建、支持 `upper=automatic`，且普通欧式与连续单障碍的均匀网格终端收益按节点取值：

```sh
python3 benchmarks/quantlib/compare_fd_accuracy.py --baseline-executable /path/to/fd-before-core
```

输出默认为 `calibration/fd_accuracy.json`；用 `--report` 指定复测文件。验收规则与校准探针见[有限差分说明](methodology.md#有限差分)。
