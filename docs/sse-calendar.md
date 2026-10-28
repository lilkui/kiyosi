# 上交所交易日历数据

[生成脚本](../tools/generate_sse_calendar.py)每次运行都会从上游默认分支获取 [XSHG 源文件](https://github.com/gerrymanoim/exchange_calendars/blob/HEAD/exchange_calendars/exchange_calendar_xshg.py)，直接读取 `precomputed_shanghai_holidays`，不导入或执行上游代码。记录数据的截止年份由该列表的最后一个日期确定；正式休市日期以[上交所公布的休市安排](https://www.sse.com.cn/disclosure/dealinstruc/closed/)为准。

对于上游列表尚未覆盖、且不晚于 2099 年的年份，脚本通过 `lunar-python` 获取农历节日和清明日期，再按[现行节假日政策](https://www.gov.cn/gongbao/2024/issue_11726/material/gwygb202433.pdf)作简化估算：春节自除夕起连休八天，劳动节围绕 5 月 1—2 日连休五天，国庆节为 10 月 1—7 日；元旦、清明、端午和中秋各安排三天，逢周三则仅休一天。中秋落在 10 月 1—7 日时，国庆假期延至 10 月 8 日。该规则不预测交易所临时休市或国务院未来的调整。XSHG 补充新年份后，应重新生成数据。

在仓库根目录运行以下命令，重新生成并校验：

```sh
uv run --no-project --with lunar-python==1.4.8 python tools/generate_sse_calendar.py --write
uv run --no-project --with lunar-python==1.4.8 python tools/generate_sse_calendar.py --check
```

生成结果是 [`crates/kiyosi-core/src/sse_holidays.rs`](../crates/kiyosi-core/src/sse_holidays.rs) 中按日期排序的静态 Rust 整数数组，由核心通过 `include!` 编译；运行、构建、测试和打包核心均无需 Python 或日历包。`--write` 更新该文件，`--check` 将完整生成结果与已提交文件比较。

Rust `TradingCalendar::sse()` 和Python `sse_calendar()` 消费同一份数据，年化基数为 243。对于 1991—2099 年以外的日期，日历将周一至周五视为交易日。公共 API 测试逐日核对该范围内的日历查询与生成数据。

历史数据取自 Apache-2.0 许可的 `exchange_calendars`；提取工作日休市日期并追加预测是本项目的修改。来源、归属与完整许可随 [Rust 核心](../crates/kiyosi-core/THIRD_PARTY_NOTICES.txt) 和 [Python 包](../THIRD_PARTY_NOTICES.txt) 分发。预测工具依赖 `lunar-python==1.4.8`（MIT），仅用于维护，不是核心或 Python 包的运行依赖。Rust API 可通过 `cargo doc --open --no-deps --locked` 查看。
