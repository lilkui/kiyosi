# 上交所交易日历数据

[生成脚本](../tools/generate_sse_calendar.py)每次运行都会从上游默认分支获取 [XSHG 源文件](https://github.com/gerrymanoim/exchange_calendars/blob/HEAD/exchange_calendars/exchange_calendar_xshg.py)，直接读取 `precomputed_shanghai_holidays`，不导入或执行上游代码。本文记录的默认分支为 `master`，仓库没有 `main` 分支；假期列表最新覆盖至 2026 年，与[上交所公布的休市安排](https://www.sse.com.cn/disclosure/dealinstruc/closed/)一致。

对于上游列表尚未覆盖、且不晚于 2099 年的年份，脚本通过 `lunar-python` 获取农历节日和清明日期，再按[现行节假日政策](https://www.gov.cn/gongbao/2024/issue_11726/material/gwygb202433.pdf)作简化估算：春节自除夕起连休八天，劳动节围绕 5 月 1—2 日连休五天，国庆节为 10 月 1—7 日；元旦、清明、端午和中秋各安排三天，逢周三则仅休一天。中秋落在 10 月 1—7 日时，国庆假期延至 10 月 8 日。该规则不预测交易所临时休市或国务院未来的调整。XSHG 补充新年份后，应重新生成数据。

在仓库根目录运行以下命令，重新生成并校验：

```sh
uv run --no-project --with lunar-python==1.4.8 python tools/generate_sse_calendar.py --write
uv run --no-project --with lunar-python==1.4.8 python tools/generate_sse_calendar.py --check
```

生成结果是静态 C++ 数据表，C++ 核心无需 Python 或日历包。对于 1991—2099 年以外的日期，日历将周一至周五视为交易日。
