from datetime import datetime, time

from kiyosi.instruments import Accumulator
from kiyosi.market import BlackScholesMertonParameters, PricingContext
from kiyosi.pricing import QuadratureAccumulatorEngine

start, end = datetime(2025, 1, 2, 15), datetime(2025, 2, 3, 15)
accumulator = Accumulator(
    option_type="call",
    strike=95,
    knock_out_level=110,
    daily_quantity=1,
    acceleration_factor=2,
    effective_time=start,
    expiry_time=end,
    observation_clock_time=time(15),
)

# call 表示累购；put 表示累沽，敲出方向也随之反转。
decumulator = accumulator.replace(option_type="put", strike=105, knock_out_level=90)
context = PricingContext(
    model_parameters=BlackScholesMertonParameters(
        risk_free_rate=0.02, dividend_yield=0, volatility=0.20
    ),
    spot_price=100,
    valuation_time=start,
)
engine = QuadratureAccumulatorEngine()

# 默认每个交易日观察并可能累计数量；触及敲出时终止，当次不再累计。
# 不利价格区域按 acceleration_factor 加速；价值按 daily_quantity 对应的资产单位计。
# 这是合约现金流的现值，可能为负，并非单位本金的保本票据价格。
print(f"累购价值：{engine.price(accumulator, context):.6f}")
print(f"累沽价值：{engine.price(decumulator, context):.6f}")

# 进入存续期后，输入此前累计且尚未结算的实际数量，不能从当前现货重建。
later = context.replace(valuation_time=datetime(2025, 1, 16, 15), spot_price=98)
live = accumulator.replace_history(accumulated_quantity=10)
print(f"此前累计 10 单位的存续价值：{engine.price(live, later):.6f}")

# terminated=True 表示此前敲出且已结算；剩余价值为零。
settled = live.replace_history(terminated=True)
print(f"已终止并结算的剩余价值：{engine.price(settled, later):.6f}")
assert engine.price(settled, later) == 0
