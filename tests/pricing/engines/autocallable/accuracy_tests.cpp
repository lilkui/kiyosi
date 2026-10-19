#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {

using namespace kiyosi;
TEST_CASE("Autocallable default grids preserve core settings", "[autocallable-accuracy]")
{
    const auto defaults = FiniteDifferenceAutocallableEngine{}.settings();
    CHECK(defaults.asset_step_count == 800);
    CHECK(defaults.time_step_count == 2000);
    CHECK(defaults.scheme == FiniteDifferenceScheme::crank_nicolson);
    CHECK_FALSE(defaults.asset_upper_boundary);
    const auto explicitly_shared_defaults = make_finite_difference_autocallable_engine(FiniteDifferenceSettings{}).value().settings();
    CHECK(explicitly_shared_defaults.asset_step_count == FiniteDifferenceSettings{}.asset_step_count);
    CHECK(explicitly_shared_defaults.time_step_count == FiniteDifferenceSettings{}.time_step_count);
}

TEST_CASE("Autocallable nearby barriers preserve prices and Greeks", "[autocallable-accuracy]")
{
    const auto start = test::day(2025, 1, 1), middle = test::day(2025, 7, 1), end = test::day(2026, 1, 1);
    const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, start, all_days_calendar()).value();
    const FiniteDifferenceAutocallableEngine engine;
    const auto price = [&](double barrier) {
        const auto option = make_binary_snowball_option({.maturity_coupon_rate = 0.02, .effective_date = start, .expiry_date = end, .observations = {{middle, 100.0, 0.08}, {end, barrier, 0.08}}}).value();
        const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(result);
        return *result;
    };
    const auto expected = price(100.0);
    const double maximum = 1.08 * std::exp(-0.04);
    for (const double barrier : {std::nextafter(100.0, 200.0), 100.0 + 1e-12, 100.0 + 1e-8, 100.0 + 1e-4}) {
        CAPTURE(barrier);
        const auto actual = price(barrier);
        CHECK(actual.price() <= maximum);
        CHECK(std::abs(actual.price() - expected.price()) <= 1e-7);
        for (const auto greek : {Greek::delta, Greek::gamma})
            CHECK(std::abs(test::greek_value(actual, greek) - test::greek_value(expected, greek)) <= 1e-7);
    }
}

TEST_CASE("Autocallable near-expiry Greeks match an analytic digital", "[autocallable-accuracy]")
{
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto option = make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}}).value();
    const auto digital = make_cash_or_nothing_option(OptionType::call, 100.0, 0.1, start, end).value();
    const FiniteDifferenceAutocallableEngine engine;
    for (const double volatility : {0.01, 0.2}) {
        for (const int seconds : {3600, 600, 60, 1}) {
            const auto valuation = start_of_day(end) - std::chrono::seconds{seconds};
            const double time = *year_fraction(valuation, start_of_day(end));
            const double width = 100.0 * volatility * std::sqrt(time);
            for (const double spot : {100.0, 100.0 + 0.5 * width}) {
                CAPTURE(volatility, seconds, spot);
                const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, volatility), spot, valuation, all_days_calendar()).value();
                const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                const auto expected = AnalyticDigitalEngine{}.price_with_greeks(digital, context, {Greek::delta, Greek::gamma});
                REQUIRE(actual);
                REQUIRE(expected);
                CHECK(std::abs(actual->price() - (std::exp(-0.04 * time) + expected->price())) <= 1e-5);
                for (const auto& [greek, absolute] : {std::pair{Greek::delta, 1e-6}, {Greek::gamma, 1e-7}}) {
                    const double reference = test::greek_value(*expected, greek);
                    CHECK(std::abs(test::greek_value(*actual, greek) - reference) <= absolute + 0.005 * std::abs(reference));
                }
            }
        }
    }
}

TEST_CASE("Autocallable coarse grids preserve positive payoff bounds and flat Greeks", "[autocallable-accuracy]")
{
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto option = make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}}).value();
    for (const int seconds : {1, 3600}) {
        const auto context = make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.2), 350.0, start_of_day(end) - std::chrono::seconds{seconds}, all_days_calendar()).value();
        for (const int count : {3, 4, 5, 10, 20, 40}) {
            CAPTURE(seconds, count);
            const auto engine = make_finite_difference_autocallable_engine({count, 2000}).value();
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            const auto scalar = engine.price(option, context);
            REQUIRE(result);
            REQUIRE(scalar);
            CHECK(result->price() >= 1.0 - 1e-10);
            CHECK(result->price() <= 1.1 + 1e-10);
            CHECK(std::abs(result->price() - 1.1) <= 1e-6);
            CHECK(*scalar == result->price());
            CHECK(std::abs(test::greek_value(*result, Greek::delta)) <= 1e-8);
            CHECK(std::abs(test::greek_value(*result, Greek::gamma)) <= 1e-8);
        }
    }
}

TEST_CASE("Autocallable terminal digital separates space time domain and barrier placement errors", "[autocallable-accuracy]")
{
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = make_pricing_context(*make_bsm_parameters(0.05, 0.01, 0.2), 100.0, start).value();
    const auto error = [&](FiniteDifferenceSettings settings, double barrier = 100.0) {
        const auto option = make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, barrier, 0.1}}}).value();
        const auto digital = make_cash_or_nothing_option(OptionType::call, barrier, 0.1, start, end).value();
        const double expected = std::exp(-0.05) + *AnalyticDigitalEngine{}.price(digital, context);
        const auto actual = make_finite_difference_autocallable_engine(settings).value().price(option, context);
        REQUIRE(actual);
        CAPTURE(settings.asset_step_count, settings.time_step_count, settings.asset_upper_boundary, barrier, *actual, expected);
        return std::abs(*actual - expected);
    };
    SECTION("space refinement at fixed time and domain")
    {
        const double coarse = error({200, 2000, FiniteDifferenceScheme::crank_nicolson, 400.0});
        for (const int count : {400, 800, 1600}) {
            const double refined = error({count, 2000, FiniteDifferenceScheme::crank_nicolson, 400.0});
            CHECK(refined <= coarse + 1e-7);
            if (count == 1600) CHECK(refined <= 1e-5);
        }
    }
    SECTION("time refinement at fixed space and domain")
    {
        for (const int count : {100, 400, 1600})
            CHECK(error({1600, count, FiniteDifferenceScheme::crank_nicolson, 400.0}) <= 1e-5);
    }
    SECTION("domain expansion within one concentration family")
    {
        for (const double upper : {500.0, 700.0, 900.0})
            CHECK(error({1600, 2000, FiniteDifferenceScheme::crank_nicolson, upper}) <= 1e-5);
    }
    SECTION("barrier placement through a former uniform mesh cell")
    {
        for (const double barrier : {99.75, 99.875, 100.0, 100.125, 100.25})
            CHECK(error({800, 2000, FiniteDifferenceScheme::crank_nicolson, 400.0}, barrier) <= 1e-5);
    }
}

TEST_CASE("Autocallable projection preserves exact current-event barrier comparisons", "[autocallable-accuracy]")
{
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto option = make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.0, 100.0}}}).value();
    const FiniteDifferenceAutocallableEngine engine;
    for (const auto& [spot, expected] : std::array<std::pair<double, double>, 5>{{{100.0, 1.08}, {std::nextafter(100.0, 0.0), 1.0}, {80.0, 1.0}, {std::nextafter(80.0, 0.0), std::nextafter(80.0, 0.0) / 100.0}, {std::nextafter(100.0, 200.0), 1.08}}}) {
        CAPTURE(spot);
        const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), spot, end, all_days_calendar()).value();
        const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(result);
        CHECK(result->price() == expected);
        CHECK_FALSE(result->has(Greek::delta));
        CHECK_FALSE(result->has(Greek::gamma));
    }
}

TEST_CASE("Autocallable daily calendars preserve a cash-carry portfolio across event restarts", "[autocallable-accuracy]")
{
    const auto start = test::day(2024, 1, 1), end = test::day(2025, 1, 1);
    constexpr double rate = 0.04;
    const auto carry_rate = [&](Date day) {
        const double time = *year_fraction(start, day);
        return std::expm1(rate * time) / time;
    };
    std::vector<AutocallableObservation> observations;
    observations.reserve(4);
    for (const auto date : {test::day(2024, 4, 1), test::day(2024, 7, 1), test::day(2024, 10, 1), end})
        observations.push_back({date, 110.0, carry_rate(date)});
    const auto option = make_ternary_snowball_option({.maturity_coupon_rate = carry_rate(end), .minimum_coupon_rate = carry_rate(end), .knock_in_level = 75.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}).value();
    for (const auto& calendar : {all_days_calendar(), weekdays_calendar()}) {
        const auto context = make_pricing_context(*make_bsm_parameters(rate, 0.01, 0.2), 100.25, start, calendar).value();
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::explicit_euler}) {
            CAPTURE(scheme);
            const auto engine = make_finite_difference_autocallable_engine({80, 2000, scheme, 400.0}).value();
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            CHECK(std::abs(result->price() - 1.0) <= 1e-5);
            CHECK(std::abs(test::greek_value(*result, Greek::delta)) <= 1e-6);
            CHECK(std::abs(test::greek_value(*result, Greek::gamma)) <= 1e-7);
            CHECK(engine.price(option, context) == result->price());
        }
    }
}

} // namespace
