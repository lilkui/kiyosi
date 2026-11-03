from datetime import UTC, date, datetime, timedelta
from functools import cache

from kiyosi import instruments
from kiyosi.market import (
    BlackScholesMertonParameters,
    PricingContext,
    weekdays_calendar,
)
from quantlib_reference import structured


@cache
def references():
    return tuple(structured.rows())


def contract(row):
    inputs = row["inputs"]
    number = lambda key: float(inputs[key])
    common = {
        key: date.fromisoformat(inputs[key])
        for key in ("effective_date", "expiry_date")
    }
    if row["instrument"] == "Accumulator":
        return instruments.Accumulator(
            **common,
            **{
                key: number(key)
                for key in (
                    "strike",
                    "knock_out_level",
                    "daily_quantity",
                    "acceleration_factor",
                    "accumulated_quantity",
                )
            },
        )
    phoenix = row["instrument"] == "PhoenixOption"
    common.update(
        principal_ratio=number("principal_ratio"),
        barrier_state=inputs["history"],
        observations=instruments.autocallable_observations(
            [common["expiry_date"]],
            [number("knock_out_level")],
            **(
                {"coupon_barrier_levels": [number("coupon_barrier_level")]}
                if phoenix
                else {"knock_out_coupon_rates": [number("coupon_rate")]}
            ),
        ),
    )
    if row["instrument"] != "BinarySnowballOption":
        common.update(
            knock_in_level=number("knock_in_level"),
            knock_in_observation_mode=inputs["knock_in_mode"],
        )
    if row["instrument"] in ("SnowballOption", "PhoenixOption"):
        common.update(
            {
                key: number(key)
                for key in ("initial_spot", "upper_strike", "lower_strike")
            }
        )
    if phoenix:
        common["coupon_rate"] = number("coupon_rate")
    else:
        common["maturity_coupon_rate"] = number("maturity_coupon_rate")
    if row["instrument"] == "TernarySnowballOption":
        common["minimum_coupon_rate"] = number("minimum_coupon_rate")
    return getattr(instruments, row["instrument"])(**common)


def market(row):
    inputs = row["inputs"]
    return PricingContext(
        model_parameters=BlackScholesMertonParameters(
            risk_free_rate=float(inputs["rate"]),
            dividend_yield=float(inputs["dividend"]),
            volatility=float(inputs["volatility"]),
        ),
        spot_price=float(inputs["spot"]),
        valuation_time=datetime.combine(
            date.fromisoformat(inputs["valuation"]), datetime.min.time(), UTC
        )
        + timedelta(hours=int(inputs["valuation_hour"])),
        calendar=weekdays_calendar(),
    )


def settings(row):
    inputs = row["inputs"]
    result = {"asset_step_count": int(inputs["asset_step_count"])}
    if row["engine"].startswith("FiniteDifference"):
        result.update(
            time_step_count=int(inputs["time_step_count"]),
            asset_upper_boundary=float(inputs["asset_upper_boundary"]),
            scheme=inputs["scheme"],
        )
    else:
        result["integration_standard_deviations"] = float(
            inputs["integration_standard_deviations"]
        )
    return result
