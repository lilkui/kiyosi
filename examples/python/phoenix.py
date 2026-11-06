from datetime import datetime, time

from kiyosi.instruments import (
    PhoenixOption,
    autocallable_observations,
    fixed_coupon_note,
)
from kiyosi.market import BlackScholesMertonParameters, PricingContext, monthly_schedule
from kiyosi.pricing import FiniteDifferenceAutocallableEngine, implied_phoenix_coupon

start, end = datetime(2025, 1, 2, 15), datetime(2025, 7, 2, 15)
times = monthly_schedule(
    start_time=start,
    end_time=end,
    first_observation_month=1,
    observation_clock_time=time(15),
)

# Phoenix 的付息障碍与敲出障碍分开设置；这里只在观察现货 >= 90 时付息。
# coupon_rate 是年化利率；每期按对应期间计息。敲入按交易日 15:00 观察。
phoenix = PhoenixOption(
    coupon_rate=0.12,
    initial_spot=100,
    knock_in_level=80,
    upper_strike=100,
    lower_strike=100,
    observation_clock_time=time(15),
    effective_time=start,
    expiry_time=end,
    observations=autocallable_observations(
        times, [103] * len(times), coupon_barrier_levels=[90] * len(times)
    ),
)

# FCN 工厂返回 Phoenix 合约：无条件固定每期票息，到期才观察敲入。
# coupon_per_period=0.01 表示每期本金的 1%，不再按天数年化。
fcn = fixed_coupon_note(
    coupon_per_period=0.01,
    initial_spot=100,
    knock_in_level=80,
    knock_out_level=103,
    observation_times=times,
    effective_time=start,
    expiry_time=end,
)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.02, dividend_yield=0, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
engine = FiniteDifferenceAutocallableEngine()

# 两种结构都返回单位本金价值，求解值保留各自的年化／每期票息单位。
for name, note, unit in (("Phoenix", phoenix, "年化"), ("FCN", fcn, "每期")):
    print(f"{name} 单位本金价格：{engine.price(note, context):.6f}")
    target = 1.0
    coupon = implied_phoenix_coupon(
        engine, note, context, target, lower_bound=0, upper_bound=0.5
    )
    repriced = engine.price(note.with_coupon(coupon), context)
    print(f"{name} 平价票息：{coupon:.4%}（{unit}），回代价格：{repriced:.8f}")
    assert abs(repriced - target) < 1e-7
