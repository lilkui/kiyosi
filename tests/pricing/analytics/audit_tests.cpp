#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"
#include "pricing/detail/math.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;
using kiyosi::test::greek_value;

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

TEST_CASE("Implied volatility counts only future Phoenix coupons", "[pricing-api][audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto fixing = day(2025, 1, 3);
    const auto end = day(2025, 1, 6);
    const ImpliedVolatilitySettings bounds{.lower_bound = 0.05, .upper_bound = 0.4};
    for (const auto valuation : {day(2025, 1, 2), fixing, day(2025, 1, 4)}) {
        for (const double lower : {100.0, 60.0}) {
            CAPTURE(valuation, lower);
            const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.05), 100.0, valuation, all_days_calendar());
            const auto option = *make_phoenix_option(
                {.coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {110.0}, .coupon_barrier_levels = {90.0}, .upper_strike = 100.0, .lower_strike = lower, .observation_dates = {fixing}, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end});
            const auto check = [&](const auto& engine) {
                const auto quote = engine.price(option, context);
                REQUIRE(quote);
                const auto result = implied_volatility(engine, option, context, *quote, bounds);
                if (valuation < fixing || lower != 100.0) {
                    REQUIRE(result);
                    CHECK(*result == bounds.lower_bound);
                } else {
                    CHECK(*quote == Catch::Approx(1.0 + (valuation == fixing ? 0.1 * 2.0 / 365.0 : 0.0)).margin(1e-10));
                    REQUIRE_FALSE(result);
                    CHECK(result.error().category == ErrorCategory::unsupported_operation);
                }
            };
            check(MonteCarloPhoenixEngine{{64, 73}});
            check(FiniteDifferencePhoenixEngine{});
        }
    }
}

TEST_CASE("Fixed barrier settlements bypass irrelevant vanilla overflow", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    struct SettlementCase {
        BarrierType type;
        RebateTiming timing;
        BarrierTouchState state;
        bool scheduled;
        bool fixed;
        bool already_paid;
    };
    for (const double rate : {0.0, 0.04}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, -0.1, 0.2), 1.7e308, day(2025, 1, 2));
        for (const auto scenario : {
                 SettlementCase{BarrierType::up_and_out, RebateTiming::at_expiry, BarrierTouchState::touched, false, true, false},
                 SettlementCase{BarrierType::up_and_out, RebateTiming::at_hit, BarrierTouchState::touched, false, true, true},
                 SettlementCase{BarrierType::up_and_out, RebateTiming::at_hit, BarrierTouchState::untouched, false, true, false},
                 SettlementCase{BarrierType::up_and_in, RebateTiming::at_expiry, BarrierTouchState::untouched, true, true, false},
                 SettlementCase{BarrierType::up_and_in, RebateTiming::at_expiry, BarrierTouchState::touched, false, false, false},
                 SettlementCase{BarrierType::up_and_out, RebateTiming::at_expiry, BarrierTouchState::untouched, true, false, false}}) {
            CAPTURE(rate, scenario.type, scenario.timing, scenario.state, scenario.scheduled);
            const auto option = *make_barrier_option(
                {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = scenario.type, .rebate = 3.0, .rebate_timing = scenario.timing, .observation_mode = scenario.scheduled ? ObservationMode::scheduled : ObservationMode::continuous, .observation_dates = scenario.scheduled ? std::vector<Date>{start} : std::vector<Date>{}, .touch_state = scenario.state});
            const auto check = [&](const auto& engine) {
                const auto result = engine.price_with_greeks(option, context, {Greek::vega});
                if (!scenario.fixed) {
                    REQUIRE_FALSE(result);
                    CHECK(result.error().category == ErrorCategory::invalid_result);
                    return;
                }
                REQUIRE(result);
                const double expected = scenario.already_paid ? 0.0 : 3.0 * (scenario.timing == RebateTiming::at_hit ? 1.0 : std::exp(-rate * 364.0 / 365.0));
                CHECK(result->price() == Catch::Approx(expected));
                CHECK(greek_value(*result, Greek::vega) == 0.0);
            };
            check(AnalyticBarrierEngine{});
            check(FiniteDifferenceBarrierEngine{});
        }
        const auto missing_history = *make_barrier_option(
            {.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .rebate = 3.0});
        const auto invalid = AnalyticBarrierEngine{}.price(missing_history, context);
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
}

TEST_CASE("Quadrature retains scaled prices beyond the former tail cutoff", "[audit-fixes]")
{
    for (const double threshold : {0.0, 11.0, 11.9, 11.99, 12.0, 13.6, 30.0}) {
        const double expected = 0.5 * std::erfc(threshold / std::sqrt(2.0));
        CHECK(detail::normal_tail_integral(threshold) == Catch::Approx(expected).epsilon(1e-8).margin(0.0));
        CHECK(detail::normal_tail_integral(-threshold) == Catch::Approx(1.0 - expected).epsilon(1e-12));
    }
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.0, 0.0, 0.2);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const bool call = type == OptionType::call;
        const double spot = call ? 1e100 : 1.5e101;
        const double strike = call ? 1.5e101 : 1e100;
        const auto context = *make_pricing_context(parameters, spot, start);
        const auto option = *make_european_option(type, strike, start, end);
        const auto price = QuadratureVanillaEngine{}.price(option, context);
        REQUIRE(price);
        CHECK(*price == Catch::Approx(2.5478923549273412e57).epsilon(1e-7));
        const auto digital = *make_cash_or_nothing_option(type, strike, 1e100, start, end);
        const auto cash = QuadratureDigitalEngine{}.price(digital, context);
        REQUIRE(cash);
        const double sign = call ? 1.0 : -1.0;
        const double d2 = (std::log(spot) - std::log(strike)) / 0.2 - 0.1;
        CHECK(*cash == Catch::Approx(1e100 * 0.5 * std::erfc(-sign * d2 / std::sqrt(2.0))).epsilon(1e-8));
    }
}

TEST_CASE("Analytic and quadrature prices preserve scaled normal tails", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.0, 0.0, 0.2);
    struct TailCase {
        double ratio, vanilla, call_cash, call_asset, put_asset;
    };
    // Independent 100-digit Decimal tail integrals, scaled before rounding to double.
    for (const auto row : {TailCase{2392.274820537378, 6.592380619674553e-32, 5.3531191121506334e-33, 1.287205586953211e-29, 1.2806132063335364e-29},
                           TailCase{10000.0, 1.1367038364232515e-163, 2.6141386421114433e-165, 2.625505680475676e-161, 2.6141386421114432e-161}}) {
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(row.ratio, type);
            const bool call = type == OptionType::call;
            const double spot = call ? 1e300 : 1e300 * row.ratio;
            const double strike = call ? 1e300 * row.ratio : 1e300;
            const auto context = *make_pricing_context(parameters, spot, start);
            const auto vanilla = *make_european_option(type, strike, start, end);
            const auto cash = *make_cash_or_nothing_option(type, strike, 1e300, start, end);
            const auto asset = *make_asset_or_nothing_option(type, strike, start, end);
            const auto check = [&](const auto& engine, const auto& option, double expected) {
                const auto value = engine.price(option, context);
                REQUIRE(value);
                CHECK(*value == Catch::Approx(expected).epsilon(1e-8).margin(0.0));
                const auto joint = engine.price_with_greeks(option, context, {Greek::delta});
                REQUIRE(joint);
                CHECK(joint->price() == *value);
            };
            check(AnalyticVanillaEngine{}, vanilla, row.vanilla);
            check(QuadratureVanillaEngine{}, vanilla, row.vanilla);
            const double cash_expected = call ? row.call_cash : row.call_asset;
            const double asset_expected = call ? row.call_asset : row.put_asset;
            check(AnalyticDigitalEngine{}, cash, cash_expected);
            check(QuadratureDigitalEngine{}, cash, cash_expected);
            check(AnalyticDigitalEngine{}, asset, asset_expected);
            check(QuadratureDigitalEngine{}, asset, asset_expected);
            const auto rho = AnalyticVanillaEngine{}.price_with_greeks(vanilla, context, {Greek::rho});
            REQUIRE(rho);
            CHECK(greek_value(*rho, Greek::rho) == Catch::Approx((call ? row.call_cash * row.ratio : -row.call_asset) / 100.0).epsilon(1e-8).margin(0.0));
        }
    }
}

TEST_CASE("Analytic vanilla Greeks preserve scaled normal tails", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const AnalyticVanillaEngine engine;
    const auto parameters = *make_bsm_parameters(0.0, 0.0, 0.2);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto small = *make_pricing_context(parameters, 1e-300, start);
        const auto option = *make_european_option(type, 1e-296, start, end);
        const auto result = engine.price_with_greeks(option, small, {Greek::gamma, Greek::speed, Greek::color, Greek::zomma});
        REQUIRE(result);
        // Independent 80-digit Decimal references include the scale before rounding to double.
        CHECK(greek_value(*result, Greek::gamma) == Catch::Approx(6.035176823596672e-159).epsilon(1e-10).margin(0.0));
        CHECK(greek_value(*result, Greek::speed) == Catch::Approx(1.3805980535242903e144).epsilon(1e-10));
        CHECK(greek_value(*result, Greek::color) == Catch::Approx(-1.7524741795041444e-158).epsilon(1e-10).margin(0.0));
        CHECK(greek_value(*result, Greek::zomma) == Catch::Approx(6.396530755190127e-157).epsilon(1e-10).margin(0.0));
        const auto large = *make_pricing_context(parameters, 1e300, start);
        const auto large_option = *make_european_option(type, 1e304, start, end);
        const auto scaled = engine.price_with_greeks(large_option, large, {Greek::vega, Greek::theta});
        REQUIRE(scaled);
        CHECK(greek_value(*scaled, Greek::vega) == Catch::Approx(1.2070353647193343e-161).epsilon(1e-10).margin(0.0));
        CHECK(greek_value(*scaled, Greek::theta) == Catch::Approx(-3.306946204710505e-163).epsilon(1e-10).margin(0.0));
    }
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 2.0), 1e308, start);
    const auto at_the_money = *make_european_option(OptionType::call, 1e308, start, end);
    const auto result = engine.price_with_greeks(at_the_money, context, {Greek::gamma});
    REQUIRE(result);
    CHECK(greek_value(*result, Greek::gamma) == Catch::Approx(1.2098536225957167e-309).epsilon(1e-10).margin(0.0));
    const auto tomorrow = start + std::chrono::days{1};
    const auto limit_context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 1e155), 100.0, start);
    const auto limit_option = *make_european_option(OptionType::call, 100.0, start, tomorrow);
    const auto limit = engine.price_with_greeks(limit_option, limit_context, GreeksRequest{true});
    REQUIRE(limit);
    CHECK(limit->price() == 100.0);
    CHECK(greek_value(*limit, Greek::color) == 0.0);
    CHECK(limit->all_finite());
}

} // namespace
