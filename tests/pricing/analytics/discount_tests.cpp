#include <cmath>
#include <numbers>
#include <vector>

#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
void check_accumulator_discounted_products(kiyosi::MonteCarloBackend backend)
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1);
    struct Scenario { double spot, quantity, rate; };
    for (const auto [spot, quantity, rate] : {Scenario{100.0, 1e308, 2000.0},
                                            Scenario{1e-200, 1e-200, -335800.0},
                                            Scenario{1e-160, 1e-160, -255500.0}}) {
        for (const bool early : {false, true}) {
            const auto context = *make_pricing_context(
                *make_bsm_parameters(rate, rate - (early ? 365.0 : 0.0), 1e-20),
                spot, start, all_days_calendar());
            const double terminal_spot = spot * std::exp(early ? 1.0 : 0.0);
            const MonteCarloAccumulatorEngine engine{10, 42, backend};
            for (const double strike_ratio : {0.5, 1.0, 4.0}) {
                CAPTURE(backend, spot, quantity, rate, early, strike_ratio);
                const double strike = spot * strike_ratio;
                const auto option = *make_accumulator({.strike = strike,
                    .knock_out_level = spot * (early ? 2.0 : 10.0),
                    .daily_quantity = 0.0, .acceleration_factor = 1.0,
                    .accumulated_quantity = quantity, .effective_date = start,
                    .expiry_date = test::day(2025, 1, early ? 3 : 2)});
                const double difference = terminal_spot - strike;
                const double expected = difference == 0.0 ? 0.0 : std::copysign(
                    std::exp(std::log(quantity) + std::log(std::abs(difference)) - rate / 365.0), difference);
                const auto price = engine.price(option, context);
                REQUIRE(price);
                CHECK(*price == Catch::Approx(expected).epsilon(2e-12).margin(0.0));
                const NumericalShiftSettings shifts{.spot_shift = spot / 16.0, .rate_shift = 0.1};
                for (const auto greek : {Greek::delta, Greek::rho}) {
                    const auto result = engine.price_with_greeks(option, context, {greek}, shifts);
                    REQUIRE(result);
                    CHECK(result->price() == *price);
                    const auto value = result->require(greek);
                    REQUIRE(value);
                    const double expected_greek = greek == Greek::delta
                        ? std::exp(std::log(quantity) + (early ? 1.0 : 0.0) - rate / 365.0)
                        : std::exp(std::log(quantity) + std::log(strike) - rate / 365.0) / 36500.0;
                    CHECK(*value == Catch::Approx(expected_greek).epsilon(2e-6).margin(0.0));
                }
            }
        }
    }
    const auto option = *make_accumulator({.strike = 1.0, .knock_out_level = 1000.0,
        .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = 1e308,
        .effective_date = start, .expiry_date = test::day(2025, 1, 2)});
    const auto context = *make_pricing_context(*make_bsm_parameters(365.0, 365.0, 1e-20), 100.0, start, all_days_calendar());
    const auto overflow = MonteCarloAccumulatorEngine{10, 42, backend}.price(option, context);
    REQUIRE_FALSE(overflow);
    CHECK(overflow.error().category == ErrorCategory::invalid_result);
}

void check_monte_carlo_discount_scaling(kiyosi::MonteCarloBackend backend)
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1);
    const auto end = test::day(2026, 1, 1);
    const MonteCarloVanillaEngine engine{10'000, 2, 42, backend};
    const auto unit_context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 1.0, start);
    for (const auto type : {OptionType::call, OptionType::put}) {
        const auto unit = engine.price(*make_european_option(type, 1.0, start, end), unit_context);
        REQUIRE(unit);
        for (const double rate : {710.0, 740.0, 750.0, -750.0}) {
            CAPTURE(backend, type, rate);
            const double amount = rate > 0.0 ? 1e300 : 1e-300;
            const auto context = *make_pricing_context(*make_bsm_parameters(rate, rate, 0.2), amount, start);
            const auto price = engine.price(*make_european_option(type, amount, start, end), context);
            REQUIRE(price);
            CHECK(*price == Catch::Approx(*unit * std::exp(std::log(amount) - rate)).epsilon(2e-12).margin(0.0));
        }
    }
}

void check_trading_day_discount_scaling(kiyosi::MonteCarloBackend backend)
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1);
    const auto middle = test::day(2026, 1, 1);
    const auto end = test::day(2027, 1, 1);
    for (const double rate : {740.0, 750.0, -750.0}) {
        const double amount = rate > 0.0 ? 1e300 : 1e-300;
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, rate, 1e-20), 100.0, start);
        const double discounted = std::exp(std::log(amount) - rate);
        for (const bool early : {false, true}) {
            for (const double sign : {-1.0, 0.0, 1.0}) {
                CAPTURE(backend, rate, early, sign);
                BinarySnowballTerms terms{
                    .knock_out_coupon_rates = early ? std::vector{(sign - 1.0) * amount, 0.0} : std::vector{0.0},
                    .maturity_coupon_rate = early ? 0.0 : (sign - 1.0) * amount,
                    .knock_out_levels = early ? std::vector{80.0, 80.0} : std::vector{200.0},
                    .observation_dates = early ? std::vector{middle, end} : std::vector{middle},
                    .principal_ratio = amount,
                    .effective_date = start,
                    .expiry_date = early ? end : middle};
                const auto lower = make_binary_snowball_option(terms);
                REQUIRE(lower);
                if (early) terms.knock_out_coupon_rates.front() += amount;
                else terms.maturity_coupon_rate += amount;
                const auto upper = make_binary_snowball_option(terms);
                REQUIRE(upper);
                const MonteCarloBinarySnowballEngine engine{10, 42, backend};
                const auto price = engine.price(*lower, context);
                REQUIRE(price);
                CHECK(*price == Catch::Approx(sign * discounted).epsilon(2e-12).margin(0.0));
                const auto prices = detail::monte_carlo_coupon_prices(*lower, *upper, context, engine.settings());
                REQUIRE(prices);
                CHECK((*prices)[0] == *price);
                CHECK((*prices)[1] == Catch::Approx((sign + 1.0) * discounted).epsilon(2e-12).margin(0.0));
            }
        }
        for (const double strike : {90.0, 110.0}) {
            const auto option = *make_accumulator({.strike = strike, .knock_out_level = 120.0, .daily_quantity = 0.0, .acceleration_factor = 1.0, .accumulated_quantity = amount, .effective_date = start, .expiry_date = middle});
            const MonteCarloAccumulatorEngine engine{10, 42, backend};
            const double expected = (100.0 - strike) * discounted;
            const auto price = engine.price(option, context);
            REQUIRE(price);
            CHECK(*price == Catch::Approx(expected).epsilon(2e-12).margin(0.0));
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(result->price() == *price);
            CHECK(*result->require(Greek::delta) == Catch::Approx(discounted).epsilon(1e-8).margin(0.0));
        }
        if (rate > 0.0) {
            const auto phoenix = *make_phoenix_option({.coupon_rate = amount, .initial_spot = 100.0, .knock_in_level = 80.0, .knock_out_levels = {200.0, 200.0}, .coupon_barrier_levels = {0.0, 0.0}, .upper_strike = 100.0, .lower_strike = 100.0, .observation_dates = {middle, end}, .principal_ratio = 0.0, .effective_date = start, .expiry_date = end});
            const auto price = MonteCarloPhoenixEngine{10, 42, backend}.price(phoenix, context);
            REQUIRE(price);
            CHECK(*price == Catch::Approx(discounted).epsilon(2e-12).margin(0.0));
        }
    }
    const auto context = *make_pricing_context(*make_bsm_parameters(-750.0, -750.0, 1e-20), 100.0, start);
    const auto note = *make_binary_snowball_option({.knock_out_coupon_rates = {0.0}, .maturity_coupon_rate = 0.0, .knock_out_levels = {200.0}, .observation_dates = {middle}, .effective_date = start, .expiry_date = middle});
    const auto overflow = MonteCarloBinarySnowballEngine{10, 42, backend}.price(note, context);
    REQUIRE_FALSE(overflow);
    CHECK(overflow.error().category == ErrorCategory::invalid_result);
}
} // namespace

TEST_CASE("Accumulator Monte Carlo preserves discounted quantity products", "[discount-products]")
{
    check_accumulator_discounted_products(kiyosi::MonteCarloBackend::cpu);
}

#if KIYOSI_HAS_CUDA
TEST_CASE("CUDA accumulator Monte Carlo preserves discounted quantity products", "[cuda][discount-products]")
{
    check_accumulator_discounted_products(kiyosi::MonteCarloBackend::cuda);
}
#endif

TEST_CASE("Trading-day Monte Carlo preserves signed payments across extreme discount scales", "[discount-scaling]")
{
    check_trading_day_discount_scaling(kiyosi::MonteCarloBackend::cpu);
}

#if KIYOSI_HAS_CUDA
TEST_CASE("CUDA trading-day Monte Carlo preserves signed payments across extreme discount scales", "[cuda][discount-scaling]")
{
    check_trading_day_discount_scaling(kiyosi::MonteCarloBackend::cuda);
}
#endif

TEST_CASE("European Monte Carlo preserves prices across extreme discount scales", "[audit-fixes]")
{
    check_monte_carlo_discount_scaling(kiyosi::MonteCarloBackend::cpu);
}

#if KIYOSI_HAS_CUDA
TEST_CASE("CUDA European Monte Carlo preserves prices across extreme discount scales", "[cuda][audit-fixes]")
{
    check_monte_carlo_discount_scaling(kiyosi::MonteCarloBackend::cuda);
}
#endif

TEST_CASE("Prices survive underflowing discount factors", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1);
    const auto end = test::day(2026, 1, 1);
    constexpr double amount = 1e300;
    for (const double rate : {710.0, 740.0, 750.0}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, rate, 0.2), amount, start);
        const double discounted = std::exp(std::log(amount) - rate);
        for (const auto type : {OptionType::call, OptionType::put}) {
            const double sign = type == OptionType::call ? 1.0 : -1.0;
            const auto check = [&](const auto& option, double expected, const auto& analytic, const auto& quadrature) {
                for (const auto& result : {analytic.price(option, context), quadrature.price(option, context)}) {
                    CAPTURE(rate, type);
                    REQUIRE(result);
                    CHECK(*result == Catch::Approx(expected).epsilon(1e-10).margin(0.0));
                }
            };
            check(*make_cash_or_nothing_option(type, amount, amount, start, end),
                  discounted * 0.5 * std::erfc(sign * 0.1 / std::numbers::sqrt2),
                  AnalyticDigitalEngine{}, QuadratureDigitalEngine{});
            check(*make_asset_or_nothing_option(type, amount, start, end),
                  discounted * 0.5 * std::erfc(-sign * 0.1 / std::numbers::sqrt2),
                  AnalyticDigitalEngine{}, QuadratureDigitalEngine{});
            check(*make_european_option(type, amount, start, end),
                  discounted * std::erf(0.1 / std::numbers::sqrt2),
                  AnalyticVanillaEngine{}, QuadratureVanillaEngine{});
        }
    }
}

TEST_CASE("Resolved barrier settlements preserve extreme discount scales", "[audit-fixes]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1);
    const auto end = test::day(2026, 1, 1);
    for (const double rate : {740.0, 750.0, -750.0}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(rate, rate, 0.2), 150.0, start);
        for (const double amount : {0.0, rate > 0.0 ? 1e300 : 1e-300}) {
            CAPTURE(rate, amount);
            const double expected = amount == 0.0 ? 0.0 : std::exp(std::log(amount) - rate);
            const auto barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .rebate = amount});
            for (const auto& result : {AnalyticBarrierEngine{}.price(barrier, context),
                                       FiniteDifferenceBarrierEngine{}.price(barrier, context)}) {
                REQUIRE(result);
                CHECK(*result == Catch::Approx(expected).epsilon(2e-12).margin(0.0));
            }
            if (amount != 0.0) {
                const auto touch = make_cash_one_touch_up(start, end, 120.0, amount, SettlementTiming::at_expiry);
                REQUIRE(touch);
                const auto result = AnalyticBinaryBarrierEngine{}.price(*touch, context);
                REQUIRE(result);
                CHECK(*result == Catch::Approx(expected).epsilon(2e-12).margin(0.0));
            }
        }
    }
}
