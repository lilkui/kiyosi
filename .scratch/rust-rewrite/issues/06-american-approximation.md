# 06: 美式近似定价

**What to build:** 用户可以在两种语言中构造美式香草期权并使用现有 Bjerksund–Stensland 近似进行定价和支持的分析。

**Blocked by:** 04: Greeks、数值敏感度与隐含波动率。

**Status:** completed

- [x] 保留美式条款、行权范围、构造验证与替换行为，Bjerksund–Stensland 使用现有公式和适用范围。
- [x] 交付既有价格、支持的 Greeks、数值敏感度及隐含波动率调用，不引入新的美式近似或静默替代算法。
- [x] 覆盖提前行权、欧式极限、到期与非法配置边界，验证结构化错误类别与失败状态。
- [x] 采用现有美式近似回归和独立参考案例验证；说明近似与精确参考之间适用的既有精度标准。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。


## 实现与验证（2026-10-27）

Rust `AmericanOption` 保留香草条款与整个存续期内可行权语义，复用核心的执行价和日期验证；`AmericanOptionChanges` 替换返回拥有值，成功和失败均保留原合约。Python `.replace()` 是候选接口，旧版美式合约只有构造接口。

`BjerksundStenslandVanillaEngine` 迁移既有 2002 公式、二元正态积分、put-call 对称、归一化幂与稳定性保护。保留显式平坦利率要求、负变换利率和不支持的行权边界拒绝，不静默替换算法。价格取近似值、立即行权值和欧式续存值的最大值。共享分析边界改名为 `OptionContract` / `PricingEngine<I>`，十个 Greeks 使用既有数值差分；波动率反求复用核心求解器、验证顺序和默认值。默认 put 求解端点可能不在近似适用范围，保留错误传播；显式有效括区间可以恢复波动率。

| 要求 | 公共 API 验证依据 |
| --- | --- |
| 条款、构造、替换、拥有值与失败状态 | Rust `american_construction_and_replacement_validate_without_mutating`；Python `test_construction_protocols_validation_order_and_ownership`、候选 `test_replacements_validate_and_preserve_originals` |
| 价格、Greeks、数值敏感度与反求 | Rust `american_price_preserves_2002_regression`、`american_analysis_defaults_selection_solving_and_failure_state`；Python `test_2002_price_regression`、`test_maturity_regressions`、`test_greeks_defaults_scalar_differences_and_shared_solving` |
| 提前行权、欧式极限、到期与估值边界 | Rust `american_early_exercise_and_european_limits`、`american_expiry_and_exercise_window_are_inclusive`；Python `test_early_exercise_and_european_limits`、`test_expiry_and_microsecond_exercise_boundaries` |
| 适用范围、错误类别、验证时机与失败状态 | Rust `american_applicability_and_error_order_are_preserved`；Python `test_applicability_and_error_order`、`test_analysis_rejects_protocols_and_unsupported_pairs`，含节点曲线、负变换利率、不支持的边界、原生转换错误和 bool/整数窄化拒绝 |
| 既有数值回归与独立参考 | Rust `american_numerical_stability_regressions`；Python `test_small_volatility_and_extreme_monetary_regressions`、`test_live_quantlib_refinement_and_existing_error_budgets`，复用全部美式 QuantLib 1.43 场景、800/1600/3200 网格与既有稳定性检查；价格预算 0.15，数值 Greeks 采用原 profile 预算加参考 uncertainty；原近似价格回归误差 1e-11 |
| 语言公共接口、导出、类型与用法 | Python `test_installed_version_and_typed_exports`；`tests/rust-consumer` 覆盖美式价格、Greeks、反求和替换；`docs/rust-candidate.md` 与双语 README 同步 |
| 隔离验收和旧实现保留 | 完整候选测试通过，独立 legacy 环境运行相同美式共享测试；旧 C++/Python 实现和根构建配置保持原样，无新增算法、依赖、性能门槛或报告 |

- Windows 核心 `cargo test`：46 个公共 API 测试及 13 个 rustdoc 示例通过。核心测试、文档、Cargo 包验证在无效 `PYO3_PYTHON` 下通过；独立下游消费检查通过。
- CPython 3.15.1 候选完整套件：67 个测试通过；legacy 共享美式套件：10 个测试通过。审查后新增的默认 put 端点传播断言在 Rust 和两个 Python 环境中单独重跑通过。
- 旧版完整 `python -m unittest discover -s tests/python -p 'test_*.py'`：488 个测试通过，5 个跳过。
- `cargo fmt --all --check`、workspace Clippy（拒绝警告）、Ruff 检查/格式、`ty check rust/python/python` 通过。
- 候选 wheel 构建和安装通过；sdist 在临时解压目录重建 wheel，并在新的隔离环境安装，安装版本/公开类型检查和独立美式 Python 用法示例执行通过。
- 未验证 Linux、其他 CPython 版本或最低 Rust 1.85；未切换正式交付或发布。

## Standards

独立审查 0 项问题。核心语义与默认值、Python 严格边界转换、拥有值和 GIL 释放范围符合仓库规范；无额外公式重构或抽象。

## Spec

独立审查无阻塞项。建议补齐 Python 极小波动率、极端金额尺度与尾部回归，以及默认 put 反求端点的错误传播；均已补齐并通过验证。剩余 0 项。
