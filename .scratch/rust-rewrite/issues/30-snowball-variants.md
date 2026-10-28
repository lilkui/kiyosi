# 30: 雪球产品变体

**What to build:** 用户可以使用现有二元、三元雪球及命名预设，展开相同领域条款并通过积分引擎定价。

**Blocked by:** 28: 基础雪球与数值积分。

**Status:** completed

- [x] 核对并迁移所有既有二元、三元雪球和命名预设，逐项覆盖现有敲入观察、票息与敲出阶梯、行权和本金配置。
- [x] 预设只在核心中展开并复用权威领域构造与验证，Python 不重复拼装规则或定义默认值。
- [x] 各变体通过现有积分引擎获得价格及已有分析操作，历史、观察和替换失败保证保持一致。
- [x] 逐项比较预设与等价显式条款的结果，并迁移既有支付、参数与数值回归；不得只以预设名称存在视为完成。
- [x] 通过 Rust 与 Python 公共 API 验收本票能力，等价领域场景使用同一核心语义与默认值；适配层只处理协议、转换、异常和所有权，导出、类型声明与用法文档同步。
- [x] 迁移本票适用的现有契约与数值回归，覆盖合法和非法输入、验证时机、稳定错误类别及失败状态；Python 表示错误保留原生异常并拒绝意外有损转换。
- [x] 本票与已完成切片在隔离候选环境中通过测试，旧实现继续用于迁移对照；不提前删除旧实现，不新增性能回归门槛或性能报告要求。


## Comments

### 完成核验（2026-10-28）

- 实现提交：`feat: add Rust snowball variants and presets`。
- 新增 Rust `BinarySnowballTerms` / `BinarySnowballOption`、`TernarySnowballTerms` / `TernarySnowballOption`；二元无敲入监测，三元按敲入状态支付固定到期/最低票息，复用权威观察、历史、验证、替换和积分引擎。
- `SnowballPresetTerms::expand(SnowballPreset)` 在核心展开全部八项既有预设：standard、european、step_down、both_down、dual_coupon、parachute、otm、loss_capped。Python 同名构造函数仅处理表示转换、异常和默认值适配，返回普通 `SnowballOption`；导出、类型声明、Rust 示例、中英文 README 和用法文档同步。
- Rust/Python 公共 API 逐项比较预设和显式条款；迁移独立 QuantLib 终端组合、带符号票息和非单位本金、严格敲入与含等号敲出、历史和失败状态、非法输入及验证顺序、十项 Greeks、隐含波动率、固定现金流不可识别、临近到期一秒、空间/积分域收敛和并发读取场景。
- Standards 审查无文档规范违规，Spec 审查无遗漏、错误或范围扩张。一个可选维护建议为合并 Python 观察属性转换；保留既有绑定写法，不引入额外抽象。
- Linux CPython 3.15.1 核验通过：Rust 全套 327 项（含 doctest），隔离候选 Python 全套 356 项；rustfmt、Clippy（拒绝警告）、Ruff、ty、严格 rustdoc、独立 Cargo 核心打包、禁用 Python 发现的 Rust 消费者检查与新增 Python 用法示例通过。
- release wheel 和 sdist 构建通过；sdist 在独立目录重建 wheel，新环境安装后通过全部 8 项变体测试。
- 旧实现保留，按仓库要求未运行旧 C++/Python 套件；未验证 Windows、其他 CPython 版本及最低 Rust 1.85，未新增性能门槛、发布或切换交付。
