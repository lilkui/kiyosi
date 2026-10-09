#include <array>
#include <numbers>
#include <vector>

#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"
#include "pricing/detail/math.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;
using kiyosi::test::greek_value;

TEST_CASE("Analytic barriers reject incomplete future monitoring windows", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto fixing = day(2025, 1, 2);
    const auto end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 100.0, start);
    const auto barrier = *make_barrier_option(
        {.option_type = OptionType::call, .strike = 100.0, .effective_date = start,
         .expiry_date = end, .barrier_level = 110.0, .barrier_type = BarrierType::up_and_out,
         .observation_mode = ObservationMode::scheduled, .observation_dates = {fixing}});
    const auto binary = *make_cash_binary_barrier_option(
        {.option_type = OptionType::call, .strike = 100.0, .effective_date = start,
         .expiry_date = end, .barrier_level = 110.0, .barrier_type = BarrierType::up_and_in,
         .observation_mode = ObservationMode::scheduled, .observation_dates = {fixing}}, 1.0);
    const auto touch = *make_cash_one_touch_up(start, end, 110.0, 1.0,
        SettlementTiming::at_expiry, ObservationMode::scheduled, {fixing});
    for (const auto result : {AnalyticBarrierEngine{}.price(barrier, context),
                              AnalyticBinaryBarrierEngine{}.price(binary, context),
                              AnalyticBinaryBarrierEngine{}.price(touch, context)}) {
        REQUIRE_FALSE(result);
        CHECK(result.error().category == ErrorCategory::unsupported_operation);
    }
    const auto exact = FiniteDifferenceBarrierEngine{}.price(barrier, context);
    REQUIRE(exact);
    CHECK(*exact == Catch::Approx(7.965567455405804).margin(0.02));
}

TEST_CASE("Arithmetic Asians retain small moment variance across averaging windows", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const TurnbullWakemanArithmeticAveragePriceEngine engine;
    for (const auto type : {OptionType::call, OptionType::put}) {
        for (const double spot : {100.0, 1e8, 1e14}) {
            for (const double sigma : {1e-13, 1e-8, 1e-4, 0.009999, 0.01, 0.010001, 0.2}) {
                for (const int window : {0, 1, 2}) {
                    CAPTURE(type, spot, sigma, window);
                    const auto valuation = window == 2 ? start + std::chrono::days{120} : start;
                    const auto averaging = window == 1 ? start + std::chrono::days{120} : start;
                    const double delta = window == 0 ? 1.0 : 245.0 / 365.0;
                    const double lead = window == 1 ? 120.0 / 365.0 : 0.0;
                    const double scale = window == 2 ? delta : 1.0;
                    // Integrating the zero-carry second moment gives 2*sum(h^n/(n+2)!).
                    const double h = sigma * sigma * delta;
                    double term = h / 3.0;
                    double excess = term;
                    for (int n = 2; n <= 16; ++n) {
                        term *= h / (n + 2);
                        excess += term;
                    }
                    const double variance = sigma * sigma * lead + std::log1p(excess);
                    const double expected = scale * spot * std::erf(std::sqrt(variance) / (2.0 * std::sqrt(2.0)));
                    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), spot, valuation);
                    const auto option = *make_arithmetic_average_option(type, spot, start, averaging, end, window == 2 ? spot : 0.0);
                    const auto price = engine.price(option, context);
                    REQUIRE(price);
                    CHECK(*price == Catch::Approx(expected).epsilon(1e-9).margin(1e-24));
                }
            }
        }
    }
}

TEST_CASE("Arithmetic Asian small-variance prices preserve nonzero carry", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    struct Case {
        double rate;
        int window;
        double strike;
        double call;
        double put;
    };
    // Independent 75-digit evaluation of the integrated moments and normal CDF.
    const std::array cases{
        Case{0.02, 0, 101006700.13377905, 0.22861156554028925, 0.2286115636758159},
        Case{-0.02, 0, 99006633.4662235, 0.23206658954210946, 0.2320665939728782},
        Case{0.3, 0, 116619602.52533437, 0.2064505517413919, 0.20645055603932333},
        Case{-0.3, 0, 86393926.43942738, 0.2585644971473165, 0.25856449358383626},
        Case{1.4, 1, 262916196.58168963, 0.20160068921082872, 0.2016006909574677},
        Case{-1.4, 2, 76395650.08983347, 0.1859456384724795, 0.18594565344673475},
    };
    for (const auto& test : cases) {
        for (const auto type : {OptionType::call, OptionType::put}) {
            CAPTURE(test.rate, test.window, type);
            const auto valuation = test.window == 2 ? start + std::chrono::days{120} : start;
            const auto averaging = test.window == 1 ? start + std::chrono::days{120} : start;
            const auto context = *make_pricing_context(*make_bsm_parameters(test.rate, 0.0, 1e-8), 1e8, valuation);
            const auto option = *make_arithmetic_average_option(type, test.strike, start, averaging, end, test.window == 2 ? 1e8 : 0.0);
            const auto price = TurnbullWakemanArithmeticAveragePriceEngine{}.price(option, context);
            REQUIRE(price);
            CHECK(*price == Catch::Approx(type == OptionType::call ? test.call : test.put).epsilon(0.0).margin(5e-7));
        }
    }
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 1e-8), 100.0, end - std::chrono::days{1});
    const auto oversized = *make_arithmetic_average_option(OptionType::call, 1e308, start, start, end, 1.0);
    const auto rejected = TurnbullWakemanArithmeticAveragePriceEngine{}.price(oversized, context);
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("Quadrature vanilla retains small positive volatility time value", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    for (const auto type : {OptionType::call, OptionType::put}) {
        for (const double spot : {100.0, 1e14}) {
            for (const double sigma : {1e-13, 1e-12, 1e-11, 1e-6}) {
                CAPTURE(type, spot, sigma);
                const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), spot, start);
                const auto option = *make_european_option(type, spot, start, end);
                const auto price = QuadratureVanillaEngine{}.price(option, context);
                REQUIRE(price);
                const double expected = spot * std::erf(sigma / (2.0 * std::sqrt(2.0)));
                CHECK(*price == Catch::Approx(expected).epsilon(1e-12).margin(1e-24));
            }
        }
    }
}

TEST_CASE("Single-fixing geometric Asians retain European time value", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2026, 1, 1);
    const AnalyticGeometricAveragePriceEngine engine;
    for (const auto type : {OptionType::call, OptionType::put}) {
        for (const double sigma : {1e-13, 0.2}) {
            CAPTURE(type, sigma);
            const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, sigma), 1e14, start);
            const auto option = *make_geometric_average_option(type, 1e14, start, end, end);
            const auto price = engine.price(option, context);
            REQUIRE(price);
            const double expected = 1e14 * std::erf(sigma / (2.0 * std::sqrt(2.0)));
            CHECK(*price == Catch::Approx(expected).epsilon(1e-12).margin(1e-12));
            const auto invalid = engine.price(*make_geometric_average_option(type, 1e14, start, end, end, 1e14), context);
            REQUIRE_FALSE(invalid);
            CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
        }
        const auto expired = *make_pricing_context(*make_bsm_parameters(0.05, 0.02, 0.2), 110.0, end);
        const auto settled = engine.price(*make_geometric_average_option(type, 100.0, start, end, end), expired);
        REQUIRE(settled);
        CHECK(*settled == (type == OptionType::call ? 10.0 : 0.0));
    }
}

TEST_CASE("Implied volatility rejects unconditional fixed Phoenix cashflows", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto fixing = day(2025, 1, 6);
    const auto end = day(2025, 1, 10);
    const ImpliedVolatilitySettings bounds{.lower_bound = 0.05, .upper_bound = 0.4};
    for (const double rate : {0.0, 0.05}) {
        for (const auto& dates : {std::vector{end}, std::vector{fixing}, std::vector{fixing, end}}) {
            for (const double barrier : {0.0, 90.0}) {
                for (const double lower : {100.0, 60.0}) {
                    CAPTURE(rate, dates, barrier, lower);
                    const auto note = *make_phoenix_option(
                        {.coupon_rate = 0.1, .initial_spot = 100.0, .knock_in_level = 80.0,
                         .knock_out_levels = std::vector<double>(dates.size(), 120.0),
                         .coupon_barrier_levels = std::vector<double>(dates.size(), barrier),
                         .upper_strike = 100.0, .lower_strike = lower, .observation_dates = dates,
                         .knock_in_observation_mode = KnockInObservationMode::at_expiry,
                         .effective_date = start, .expiry_date = end});
                    const auto context = *make_pricing_context(*make_bsm_parameters(rate, 0.02, bounds.lower_bound), 100.0, start, all_days_calendar());
                    const bool exposed = lower != 100.0 || barrier != 0.0 || dates.size() > 1 || (rate != 0.0 && dates.front() != end);
                    const auto check = [&](const auto& engine) {
                        const auto quote = engine.price(note, context);
                        REQUIRE(quote);
                        const auto result = implied_volatility(engine, note, context, *quote, bounds);
                        if (exposed) {
                            REQUIRE(result);
                            CHECK(*result == bounds.lower_bound);
                        } else {
                            const double expected = std::exp(-rate * 9.0 / 365.0) + 0.1 *
                                static_cast<double>((dates.front() - start).count()) / 365.0 *
                                std::exp(-rate * static_cast<double>((dates.front() - start).count()) / 365.0);
                            CHECK(*quote == Catch::Approx(expected).epsilon(1e-10));
                            REQUIRE_FALSE(result);
                            CHECK(result.error().category == ErrorCategory::unsupported_operation);
                        }
                    };
                    check(MonteCarloPhoenixEngine{{64, 73}});
                    check(FiniteDifferencePhoenixEngine{});
                }
            }
        }
    }
}

TEST_CASE("Trading finite-difference prices are invariant to currency scale", "[audit-fixes]")
{
    const auto start = day(2025, 1, 1);
    const auto end = day(2025, 1, 10);
    const auto parameters = *make_bsm_parameters(0.05, 0.02, 0.2);
    const auto prices = [&](double scale) {
        const auto context = *make_pricing_context(parameters, 100.0 * scale, start, all_days_calendar());
        const auto accumulator = *make_accumulator(
            {.strike = 100.0 * scale, .knock_out_level = 120.0 * scale, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
        const auto snowball = *make_binary_snowball_option(
            {.knock_out_coupon_rates = {0.1}, .maturity_coupon_rate = 0.2, .knock_out_levels = {101.0 * scale}, .observation_dates = {end}, .effective_date = start, .expiry_date = end});
        const auto accrued = FiniteDifferenceAccumulatorEngine{}.price(accumulator, context);
        const auto normalized = FiniteDifferenceBinarySnowballEngine{}.price(snowball, context);
        REQUIRE(accrued);
        REQUIRE(normalized);
        return std::array{*accrued / scale, *normalized};
    };
    const auto expected = prices(1.0);
    for (const double scale : {0.01, 0.0001, 0.000001}) {
        CAPTURE(scale);
        const auto actual = prices(scale);
        for (std::size_t index = 0; index < expected.size(); ++index)
            CHECK(actual[index] == Catch::Approx(expected[index]).epsilon(0.0).margin(1e-9));
    }
}

TEST_CASE("Numerical time Greeks preserve microsecond shifts", "[audit-fixes]")
{
    const auto parameters = *make_bsm_parameters(0.05, 0.02, 0.2);
    for (const int year : {1900, 2026, 9999}) {
        const auto end = day(year, 1, 1);
        const auto option = *make_european_option(OptionType::call, 100.0, day(year - 1, 1, 1), end);
        for (const int microseconds : {1, 10}) {
            CAPTURE(year, microseconds);
            const auto shift = std::chrono::microseconds{microseconds};
            const auto context = *make_pricing_context(parameters, 100.0, start_of_day(end) - shift);
            const auto before = *make_pricing_context(parameters, 100.0, start_of_day(end) - 2 * shift);
            const double elapsed_days = std::chrono::duration<double, std::ratio<86400>>{2 * shift}.count();
            const auto check = [&](const auto& engine, const auto& result) {
                const auto prior = engine.price(option, before);
                REQUIRE(prior);
                REQUIRE(*prior > 0.0);
                REQUIRE(result);
                CHECK(greek_value(*result, Greek::theta) == Catch::Approx(-*prior / elapsed_days).epsilon(1e-10));
            };
            const AnalyticVanillaEngine analytic;
            check(analytic, calculate_numerical_greeks(analytic, option, context));
            const QuadratureVanillaEngine quadrature;
            check(quadrature, quadrature.price_with_greeks(option, context, {Greek::theta}));
        }
    }
}

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
        const double expected = 0.5 * std::erfc(threshold / std::numbers::sqrt2);
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
