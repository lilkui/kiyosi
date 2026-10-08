#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;

TEST_CASE("Implied volatility rejects fixed participation and zero accrual", "[pricing-api][audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto fixing = day(2025, 7, 1);
    const auto end = day(2026, 1, 1);
    const ImpliedVolatilitySettings bounds{.lower_bound = 0.05, .upper_bound = 0.4};
    const auto check = [&](const auto& engine, const auto& option, const auto& context, bool exposed) {
        const auto quote = engine.price(option, context);
        REQUIRE(quote);
        const auto result = implied_volatility(engine, option, context, *quote, bounds);
        if (exposed) {
            REQUIRE(result);
            CHECK(*result == bounds.lower_bound);
        } else {
            REQUIRE_FALSE(result);
            CHECK(result.error().category == ErrorCategory::unsupported_operation);
        }
    };
    for (const double rate : {0.0, 0.04}) {
        for (const double lower : {100.0, 60.0}) {
            for (const double coupon : {0.0, 0.1}) {
                CAPTURE(rate, lower, coupon);
                const auto context = *make_pricing_context(*make_bsm_parameters(rate, 0.0, 0.05), 100.0, start);
                const bool exposed = rate != 0.0 || lower != 100.0 || coupon != 0.0;
                const auto snowball = *make_snowball_option(
                    {.knock_out_coupon_rates = {coupon, coupon}, .maturity_coupon_rate = coupon, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0, 120.0}, .upper_strike = 100.0, .lower_strike = lower, .observation_dates = {fixing, end}, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .effective_date = start, .expiry_date = end});
                check(MonteCarloSnowballEngine{{64, 73}}, snowball, context, exposed);
                check(FiniteDifferenceSnowballEngine{}, snowball, context, exposed);
                const auto phoenix = *make_phoenix_option(
                    {.coupon_rate = coupon, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0, 120.0}, .coupon_barrier_levels = {90.0, 90.0}, .upper_strike = 100.0, .lower_strike = lower, .observation_dates = {fixing, end}, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .effective_date = start, .expiry_date = end});
                check(MonteCarloPhoenixEngine{{64, 73}}, phoenix, context, exposed);
                check(FiniteDifferencePhoenixEngine{}, phoenix, context, exposed);
            }
        }
    }
    for (const bool historical : {false, true}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.05), historical ? 100.0 : 70.0,
                                                   historical ? fixing : start);
        const auto note = *make_snowball_option(
            {.knock_out_coupon_rates = {0.0, 0.0}, .maturity_coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {120.0, 120.0}, .upper_strike = 100.0, .lower_strike = 100.0, .observation_dates = {fixing, end}, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = historical ? AutocallableBarrierState::knocked_in : AutocallableBarrierState::none, .effective_date = start, .expiry_date = end});
        check(MonteCarloSnowballEngine{{64, 73}}, note, context, false);
        check(FiniteDifferenceSnowballEngine{}, note, context, false);
    }
    for (const double knock_out : {90.0, 100.0, 120.0}) {
        for (const double acceleration : {0.0, 1.0}) {
            for (const double quantity : {0.0, 1.0}) {
                CAPTURE(knock_out, acceleration, quantity);
                const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.05), 80.0, start);
                const auto option = *make_accumulator(
                    {.strike = 100.0, .knock_out_level = knock_out, .daily_quantity = 1.0, .acceleration_factor = acceleration, .accumulated_quantity = quantity, .effective_date = start, .expiry_date = end});
                const bool exposed = quantity != 0.0 || acceleration != 0.0 || knock_out > 100.0;
                check(MonteCarloAccumulatorEngine{{64, 73}}, option, context, exposed);
                check(FiniteDifferenceAccumulatorEngine{}, option, context, exposed);
            }
        }
    }
}

} // namespace
