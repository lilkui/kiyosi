#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <future>
#include <limits>
#include <numbers>
#include <optional>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include <kiyosi/pricing/engines/autocallable/integral.hpp>

#include "pricing/detail/autocallable_pricer.hpp"
#include "support/common.hpp"
#include "support/numerical.hpp"

namespace {
using namespace kiyosi;
using test::day;

// Closed BSM cash plus digital portfolio, independent of the quadrature implementation.
double binary_reference(double spot, double volatility, double rate, double time, double coupon = 0.1)
{
    const double d2 = (std::log(spot / 100.0) + (rate - 0.01 - 0.5 * volatility * volatility) * time) /
                      (volatility * std::sqrt(time));
    return std::exp(-rate * time) * (1.0 + coupon * 0.5 * std::erfc(-d2 / std::numbers::sqrt2));
}

template <AutocallableProduct Note>
void check_quadrature_convergence(const Note& note, const PricingContext& context)
{
    const auto normal = QuadratureAutocallableEngine{}.price(note, context);
    const auto refined = make_quadrature_autocallable_engine({1024, 8.0})->price(note, context);
    const auto expanded = make_quadrature_autocallable_engine({1024, 10.0})->price(note, context);
    const auto finite_difference = make_finite_difference_autocallable_engine({2000, 4000})->price(note, context);
    REQUIRE(normal);
    REQUIRE(refined);
    REQUIRE(expanded);
    REQUIRE(finite_difference);
    CAPTURE(*normal, *refined, *expanded, *finite_difference);
    CHECK(std::abs(*normal - *refined) <= 1e-4);
    CHECK(std::abs(*refined - *expanded) <= 1e-4);
    // Fine FD error was measured below 2e-6 on these scenarios, inside the 1bp price budget.
    CHECK(std::abs(*normal - *finite_difference) <= 1e-4);
}

TEST_CASE("Autocallable event dates preserve future observations and include expiry once", "[autocallable-event-dates]")
{
    const auto start = day(2025, 1, 6), observation = day(2025, 1, 8), end = day(2025, 1, 10);
    const auto parameters = *make_bsm_parameters(0.03, 0.01, 0.2);
    for (const bool observes_expiry : {false, true}) {
        std::vector<AutocallableObservation> observations{{start, 120.0, 0.1}, {observation, 115.0, 0.2}};
        if (observes_expiry) observations.push_back({end, 110.0, 0.3});
        const auto note = make_snowball_option({.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations});
        REQUIRE(note);
        for (const Date valuation : {start, observation})
            for (const int hour : {0, 12}) {
                CAPTURE(observes_expiry, valuation, hour);
                const auto context = *make_pricing_context(parameters, 100.0, start_of_day(valuation) + std::chrono::hours{hour});
                detail::AutocallableContract<SnowballOption> contract{*note, context.calendar()};
                REQUIRE(contract.validate(context));
                const std::size_t next = valuation == start ? 1 : 2;
                const std::vector<Date> scheduled = valuation == start ? std::vector{observation, end} : std::vector{end};
                const std::vector<Date> daily = valuation == start ? std::vector{day(2025, 1, 7), observation, day(2025, 1, 9), end}
                                                                   : std::vector{day(2025, 1, 9), end};
                CHECK(detail::autocallable_event_dates(*note, context, next, false) == scheduled);
                CHECK(detail::autocallable_event_dates(*note, context, next, true) == daily);
            }
    }
}

TEST_CASE("Quadrature multi-event defaults converge across four products and monitoring modes", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2025, 4, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.03, 0.01, 0.2), 100.0, start, all_days_calendar());
    const std::vector<AutocallableObservation> observations{{day(2025, 2, 1), 110.0, 0.08}, {day(2025, 3, 1), 108.0, 0.08}, {end, 105.0, 0.08}};
    const std::vector<AutocallableObservation> phoenix_observations{{day(2025, 2, 1), 110.0, 0.0, 95.0}, {day(2025, 3, 1), 108.0, 0.0, 95.0}, {end, 105.0, 0.0, 95.0}};
    for (const auto mode : {KnockInObservationMode::at_expiry, KnockInObservationMode::every_trading_day}) {
        CAPTURE(mode);
        check_quadrature_convergence(*make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}), context);
        check_quadrature_convergence(*make_binary_snowball_option({.maturity_coupon_rate = 0.06, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}), context);
        check_quadrature_convergence(*make_ternary_snowball_option({.maturity_coupon_rate = 0.06, .minimum_coupon_rate = 0.01, .knock_in_level = 80.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}), context);
        check_quadrature_convergence(*make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = phoenix_observations}), context);
    }
}

TEST_CASE("Quadrature monthly step-down contracts converge with seasoned history near barriers", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    std::vector<AutocallableObservation> observations;
    for (unsigned month = 2; month <= 12; ++month)
        observations.push_back({day(2025, month, 1), 110.0 - static_cast<double>(month), 0.08});
    observations.push_back({end, 97.0, 0.08});
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.35), 98.5,
                                               day(2025, 7, 15), all_days_calendar());
    check_quadrature_convergence(*make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 95.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = AutocallableBarrierState::knocked_in, .effective_date = start, .expiry_date = end, .observations = observations}), context);
    for (auto& observation : observations) {
        observation.coupon_rate = 0.0;
        observation.coupon_barrier_level = 99.0;
    }
    check_quadrature_convergence(*make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 95.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations}), context);
}

TEST_CASE("Quadrature discounts each signed Phoenix coupon at its own event", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2025, 4, 1);
    const std::vector<AutocallableObservation> observations{{day(2025, 2, 1), 200.0}, {day(2025, 3, 1), 190.0}, {end, 180.0}};
    for (const double coupon : {0.08, -0.2, 0.0}) {
        const auto note = *make_phoenix_option({.coupon_rate = coupon, .initial_spot = 100.0, .knock_in_level = 50.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = observations});
        for (const auto valuation : {start_of_day(start), start_of_day(observations.front().date),
                                     start_of_day(observations.front().date) + std::chrono::microseconds{1}}) {
            const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 1e-8), 100.0, valuation, all_days_calendar());
            double expected = std::exp(-0.04 * *year_fraction(valuation, start_of_day(end)));
            auto previous = start;
            for (const auto& observation : observations) {
                if (start_of_day(observation.date) >= valuation)
                    expected += coupon * *year_fraction(previous, observation.date) * std::exp(-0.04 * *year_fraction(valuation, start_of_day(observation.date)));
                previous = observation.date;
            }
            const auto actual = QuadratureAutocallableEngine{}.price(note, context);
            REQUIRE(actual);
            CHECK(*actual == Catch::Approx(expected).margin(1e-9));
        }
    }
}

TEST_CASE("Quadrature settings have core defaults and reject invalid budgets", "[autocallable-quadrature]")
{
    const auto defaults = QuadratureAutocallableEngine{}.settings();
    CHECK(defaults.asset_step_count == 512);
    CHECK(defaults.integration_standard_deviations == 8.0);
    CHECK(make_quadrature_autocallable_engine({4, 0.5}));
    for (const auto settings : {QuadratureSettings{3, 8.0}, {0, 8.0}, {-1, 8.0}, {32, 0.0}, {32, -1.0}, {32, std::numeric_limits<double>::infinity()}, {32, std::numeric_limits<double>::quiet_NaN()}}) {
        const auto engine = make_quadrature_autocallable_engine(settings);
        REQUIRE_FALSE(engine);
        CHECK(engine.error().category == ErrorCategory::invalid_parameter);
    }
}

TEST_CASE("Quadrature integrates zero-barrier Phoenix coupons across finite-domain tails", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 2.0), 100.0, start, all_days_calendar());
    const auto note = *make_phoenix_option({.coupon_rate = -0.5, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 100.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .principal_ratio = 0.25, .effective_date = start, .expiry_date = end, .observations = {{middle, 100.0}, {end, 100.0}}});
    const double time = *year_fraction(start, middle);
    const double survival = 0.5 * std::erfc((0.03 - 2.0) * std::sqrt(time) / (2.0 * std::numbers::sqrt2));
    const double expected = (-0.5 * time + 0.25 * (1.0 - survival)) * std::exp(-0.04 * time) +
                            (0.25 - 0.5 * (1.0 - time)) * survival * std::exp(-0.04);
    for (const auto settings : {QuadratureSettings{}, QuadratureSettings{128, 0.5}}) {
        const auto actual = make_quadrature_autocallable_engine(settings)->price(note, context);
        REQUIRE(actual);
        CHECK(*actual == Catch::Approx(expected).margin(1e-10));
    }
}

TEST_CASE("Quadrature integrates four terminal portfolios in both knock-in modes", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const QuadratureAutocallableEngine engine;
    for (const auto mode : {KnockInObservationMode::at_expiry, KnockInObservationMode::every_trading_day}) {
        for (const double coupon : {0.1, -4.0}) {
            const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.8), 100.0, start, all_days_calendar());
            const double cash_digital = binary_reference(100.0, 0.8, 0.04, 1.0, coupon);
            const auto put = [&](double strike) {
                return *AnalyticVanillaEngine{}.price(*make_european_option(OptionType::put, strike, start, end), context);
            };
            const double downside = cash_digital - (put(100.0) - put(60.0)) / 100.0;
            const auto check = [&](const auto& note, double expected) {
                const auto price = engine.price(note, context);
                REQUIRE(price);
                CAPTURE(mode, coupon, expected);
                CHECK(*price == Catch::Approx(expected).margin(1e-4));
            };
            check(*make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, coupon}}}), cash_digital);
            check(*make_snowball_option({.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, coupon}}}), downside);
            check(*make_ternary_snowball_option({.maturity_coupon_rate = 0.0, .minimum_coupon_rate = 0.0, .knock_in_level = 100.0, .knock_in_observation_mode = mode, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, coupon}}}), cash_digital);
            check(*make_phoenix_option({.coupon_rate = coupon, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.0, 100.0}}}), downside);
        }
    }
}

TEST_CASE("Quadrature resolves a one-second digital without sampling its density", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}});
    const auto valuation = start_of_day(end) - std::chrono::seconds{1};
    const double time = *year_fraction(valuation, start_of_day(end));
    for (const double volatility : {0.01, 0.2}) {
        for (const double spot : {100.0, 100.0 + 50.0 * volatility * std::sqrt(time)}) {
            const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, volatility), spot, valuation, all_days_calendar());
            const auto actual = QuadratureAutocallableEngine{}.price(note, context);
            REQUIRE(actual);
            CHECK(*actual == Catch::Approx(binary_reference(spot, volatility, 0.04, time)).margin(1e-9));
        }
    }
}

TEST_CASE("Quadrature retains affine terminal tails with zero lower strike and nonunit principal", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 2.0), 20.0, start);
    const auto put = *AnalyticVanillaEngine{}.price(*make_european_option(OptionType::put, 100.0, start, end), context);
    const double expected = 0.25 * std::exp(-0.04) - put / 100.0;
    const auto snowball = *make_snowball_option({.maturity_coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 0.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .principal_ratio = 0.25, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.0}}});
    const auto phoenix = *make_phoenix_option({.coupon_rate = 0.0, .initial_spot = 100.0, .knock_in_level = 100.0, .upper_strike = 100.0, .lower_strike = 0.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .principal_ratio = 0.25, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.0, 0.0}}});
    const QuadratureAutocallableEngine engine;
    for (const auto& actual : {engine.price(snowball, context), engine.price(phoenix, context)}) {
        REQUIRE(actual);
        CHECK(*actual == Catch::Approx(expected).margin(1e-10));
    }
}

TEST_CASE("Quadrature daily knock-in preserves its distinct continuation layer", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2025, 4, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.4), 100.0, start, all_days_calendar());
    std::array<double, 2> prices{};
    for (const auto mode : {KnockInObservationMode::every_trading_day, KnockInObservationMode::at_expiry}) {
        const auto note = *make_ternary_snowball_option({.maturity_coupon_rate = 0.2, .minimum_coupon_rate = 0.0, .knock_in_level = 100.0, .knock_in_observation_mode = mode, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 2, 1), 200.0, 0.2}, {day(2025, 3, 1), 190.0, 0.2}, {end, 180.0, 0.2}}});
        const auto actual = QuadratureAutocallableEngine{}.price(note, context);
        const auto independent = make_finite_difference_autocallable_engine({2000, 4000})->price(note, context);
        REQUIRE(actual);
        REQUIRE(independent);
        CHECK(*actual == Catch::Approx(*independent).margin(4e-4));
        prices[mode == KnockInObservationMode::every_trading_day ? 0 : 1] = *actual;
    }
    CHECK(prices[0] < prices[1] - 0.01);
}

TEST_CASE("Quadrature Greeks match independent finite-bump cash digital formulas", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), valuation = day(2025, 6, 1), end = day(2026, 1, 1);
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}});
    const double time = *year_fraction(valuation, end);
    constexpr double spot = 103.0, volatility = 0.3, rate = -0.04;
    const NumericalShiftSettings shifts{.spot_shift = 0.25, .volatility_shift = 0.002, .rate_shift = 0.001, .time_shift_days = 1};
    const double h = (spot + shifts.spot_shift) - spot;
    const double v = (volatility + shifts.volatility_shift) - volatility;
    const double r = (rate + shifts.rate_shift) - rate;
    const auto value = [&](double s, double vol, double rf, double t) { return binary_reference(s, vol, rf, t); };
    const auto delta = [&](double vol, double t) { return (value(spot + h, vol, rate, t) - value(spot - h, vol, rate, t)) / (2.0 * h); };
    const auto gamma = [&](double vol, double t) { return (value(spot + h, vol, rate, t) - 2.0 * value(spot, vol, rate, t) + value(spot - h, vol, rate, t)) / (h * h); };
    const double dt = 1.0 / 365.0;
    const double center = value(spot, volatility, rate, time);
    const std::array expected{
        std::pair{Greek::delta, delta(volatility, time)},
        std::pair{Greek::gamma, gamma(volatility, time)},
        std::pair{Greek::speed, (value(spot + 2.0 * h, volatility, rate, time) - 2.0 * value(spot + h, volatility, rate, time) +
                                 2.0 * value(spot - h, volatility, rate, time) - value(spot - 2.0 * h, volatility, rate, time)) /
                                    (2.0 * h * h * h)},
        std::pair{Greek::vega, (value(spot, volatility + v, rate, time) - value(spot, volatility - v, rate, time)) / (200.0 * v)},
        std::pair{Greek::vanna, (delta(volatility + v, time) - delta(volatility - v, time)) / (200.0 * v)},
        std::pair{Greek::zomma, (gamma(volatility + v, time) - gamma(volatility - v, time)) / (200.0 * v)},
        std::pair{Greek::rho, (value(spot, volatility, rate + r, time) - value(spot, volatility, rate - r, time)) / (200.0 * r)},
        std::pair{Greek::theta, (value(spot, volatility, rate, time - dt) - value(spot, volatility, rate, time + dt)) / 2.0},
        std::pair{Greek::charm, (delta(volatility, time - dt) - delta(volatility, time + dt)) / 2.0},
        std::pair{Greek::color, (gamma(volatility, time - dt) - gamma(volatility, time + dt)) / 2.0}};
    const auto context = *make_pricing_context(*make_bsm_parameters(rate, 0.01, volatility), spot, valuation);
    const auto actual = QuadratureAutocallableEngine{}.price_with_greeks(note, context, true, shifts);
    REQUIRE(actual);
    CHECK(actual->price() == Catch::Approx(center).margin(1e-10));
    for (const auto& [greek, reference] : expected) {
        CAPTURE(greek, reference);
        CHECK(test::greek_value(*actual, greek) == Catch::Approx(reference).margin(1e-9 + 1e-5 * std::abs(reference)));
    }
}

TEST_CASE("Quadrature multi-event Greeks remain stable under grid and bump refinement", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), middle = day(2025, 6, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.25), 101.5,
                                               day(2025, 1, 15), all_days_calendar());
    const auto snowball = *make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{middle, 110.0, 0.08}, {end, 105.0, 0.08}}});
    const auto phoenix = *make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{middle, 110.0, 0.0, 95.0}, {end, 105.0, 0.0, 95.0}}});
    struct Tolerance {
        Greek greek;
        double absolute;
        double relative;
    };
    // Price units per spot, percentage point or calendar day follow NumericalShiftSettings.
    constexpr std::array tolerances{
        Tolerance{Greek::delta, 3e-6, 0.005}, Tolerance{Greek::gamma, 3e-7, 0.01},
        Tolerance{Greek::speed, 1e-7, 0.05}, Tolerance{Greek::vega, 3e-6, 0.005},
        Tolerance{Greek::vanna, 1e-7, 0.05}, Tolerance{Greek::zomma, 1e-7, 0.05},
        Tolerance{Greek::rho, 3e-6, 0.005}, Tolerance{Greek::theta, 1e-7, 0.02},
        Tolerance{Greek::charm, 1e-7, 0.05}, Tolerance{Greek::color, 1e-7, 0.05}};
    const NumericalShiftSettings coarse_bumps{.spot_shift = 0.5, .volatility_shift = 0.002, .rate_shift = 0.001};
    const NumericalShiftSettings fine_bumps{.spot_shift = 0.25, .volatility_shift = 0.001, .rate_shift = 0.0005};
    const QuadratureAutocallableEngine normal;
    const auto refined = make_quadrature_autocallable_engine({1024, 8.0}).value();
    const auto check = [&](const auto& note) {
        const auto standard = normal.price_with_greeks(note, context, true, coarse_bumps);
        const auto finer_grid = refined.price_with_greeks(note, context, true, coarse_bumps);
        const auto smaller_bumps = refined.price_with_greeks(note, context, true, fine_bumps);
        REQUIRE(standard);
        REQUIRE(finer_grid);
        REQUIRE(smaller_bumps);
        CHECK(standard->price() == Catch::Approx(finer_grid->price()).margin(1e-4));
        for (const auto& tolerance : tolerances) {
            const double reference = test::greek_value(*finer_grid, tolerance.greek);
            const double limit = tolerance.absolute + tolerance.relative * std::abs(reference);
            CAPTURE(tolerance.greek, reference, limit);
            CHECK(std::abs(test::greek_value(*standard, tolerance.greek) - reference) <= limit);
            CHECK(std::abs(test::greek_value(*smaller_bumps, tolerance.greek) - reference) <= limit);
        }
    };
    check(snowball);
    check(phoenix);
}

TEST_CASE("Quadrature terminal boundaries preserve strict knock-in and inclusive coupon knock-out", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto note = *make_phoenix_option({.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.0, 100.0}}});
    for (const auto& [spot, expected] : std::array<std::pair<double, double>, 5>{{{100.0, 1.08}, {std::nextafter(100.0, 0.0), 1.0}, {80.0, 1.0}, {std::nextafter(80.0, 0.0), std::nextafter(80.0, 0.0) / 100.0}, {60.0, 0.6}}}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), spot, end, all_days_calendar());
        const auto result = QuadratureAutocallableEngine{}.price_with_greeks(note, context, true);
        REQUIRE(result);
        CHECK(result->price() == expected);
        for (const auto greek : {Greek::delta, Greek::gamma, Greek::speed, Greek::theta, Greek::charm, Greek::color,
                                 Greek::vega, Greek::vanna, Greek::zomma, Greek::rho})
            CHECK_FALSE(result->has(greek));
    }
}

TEST_CASE("Quadrature preserves history validation and already terminated notes", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.0, middle, all_days_calendar());
    const QuadratureAutocallableEngine engine;
    for (const auto state : {std::optional<AutocallableBarrierState>{}, std::optional{AutocallableBarrierState::none},
                             std::optional{AutocallableBarrierState::knocked_out}}) {
        const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = state, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 3, 1), 110.0, 0.08}, {end, 110.0, 0.08}}});
        const auto result = engine.price_with_greeks(note, context, true);
        if (!state) {
            REQUIRE_FALSE(result);
            CHECK(result.error().category == ErrorCategory::invalid_parameter);
        } else if (*state == AutocallableBarrierState::knocked_out) {
            REQUIRE(result);
            CHECK(result->price() == 0.0);
            for (const auto greek : {Greek::delta, Greek::gamma, Greek::speed, Greek::theta, Greek::charm, Greek::color,
                                     Greek::vega, Greek::vanna, Greek::zomma, Greek::rho})
                CHECK(test::greek_value(*result, greek) == 0.0);
        } else {
            REQUIRE(result);
            CHECK(result->price() > 0.0);
        }
    }
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.05, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}});
    const auto expired = *make_pricing_context(context.model_parameters(), 100.0, end + std::chrono::days{1});
    const auto rejected = engine.price(note, expired);
    REQUIRE_FALSE(rejected);
    CHECK(rejected.error().category == ErrorCategory::invalid_time_range);
}

TEST_CASE("Quadrature applies current knock-out before continuation and retains known knock-in", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), middle = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{middle, 100.0, 0.1}, {end, 100.0, 0.1}}});
    const QuadratureAutocallableEngine engine;
    for (const double spot : {100.0, std::nextafter(100.0, 0.0)}) {
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), spot, middle, all_days_calendar());
        const auto result = engine.price_with_greeks(note, context, {Greek::delta, Greek::gamma, Greek::theta, Greek::vega, Greek::rho});
        REQUIRE(result);
        const double expected = spot == 100.0 ? 1.0 + 0.1 * *year_fraction(start, middle)
                                              : binary_reference(spot, 0.3, 0.04, *year_fraction(middle, end));
        CHECK(result->price() == Catch::Approx(expected).margin(1e-10));
        CHECK_FALSE(result->has(Greek::delta));
        CHECK_FALSE(result->has(Greek::gamma));
        CHECK_FALSE(result->has(Greek::theta));
        CHECK(result->has(Greek::vega));
        CHECK(result->has(Greek::rho));
        if (spot == 100.0) {
            CHECK(test::greek_value(*result, Greek::vega) == 0.0);
            CHECK(test::greek_value(*result, Greek::rho) == 0.0);
        }
    }
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), 90.0, end, all_days_calendar());
    for (const auto mode : {KnockInObservationMode::every_trading_day, KnockInObservationMode::at_expiry}) {
        for (const auto history : {AutocallableBarrierState::none, AutocallableBarrierState::knocked_in}) {
            const auto snowball = *make_snowball_option({.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = mode, .barrier_state = history, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}});
            const auto ternary = *make_ternary_snowball_option({.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.02, .knock_in_level = 80.0, .knock_in_observation_mode = mode, .barrier_state = history, .effective_date = start, .expiry_date = end, .observations = {{end, 110.0, 0.08}}});
            const auto snowball_price = engine.price(snowball, context);
            const auto ternary_price = engine.price(ternary, context);
            if (mode == KnockInObservationMode::at_expiry && history == AutocallableBarrierState::knocked_in) {
                REQUIRE_FALSE(snowball_price);
                REQUIRE_FALSE(ternary_price);
                CHECK(snowball_price.error().category == ErrorCategory::invalid_option);
                CHECK(ternary_price.error().category == ErrorCategory::invalid_option);
                continue;
            }
            REQUIRE(snowball_price);
            REQUIRE(ternary_price);
            CHECK(*snowball_price == (history == AutocallableBarrierState::knocked_in ? 0.9 : 1.05));
            CHECK(*ternary_price == (history == AutocallableBarrierState::knocked_in ? 1.02 : 1.05));
        }
    }
}

TEST_CASE("Quadrature implied volatility and coupon recover quotes and reject impossible targets", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}});
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), 103.0, start);
    const QuadratureAutocallableEngine engine;
    const double quote = binary_reference(103.0, 0.3, 0.04, 1.0);
    const auto volatility = implied_volatility(engine, note, context, quote, {.lower_bound = 0.1, .upper_bound = 0.5, .price_tolerance = 1e-11});
    REQUIRE(volatility);
    CHECK(*volatility == Catch::Approx(0.3).margin(1e-7));
    const auto multiple_events = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 7, 1), 100.0, 0.1}, {end, 100.0, 0.1}}});
    const auto multiple_quote = engine.price(multiple_events, context);
    REQUIRE(multiple_quote);
    const auto default_volatility = implied_volatility(engine, multiple_events, context, *multiple_quote);
    REQUIRE(default_volatility);
    CHECK(*default_volatility == Catch::Approx(0.3).margin(1e-5));
    const auto recovered = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, *default_volatility), 103.0, start);
    const auto repriced = engine.price(multiple_events, recovered);
    REQUIRE(repriced);
    CHECK(*repriced == Catch::Approx(*multiple_quote).margin(ImpliedVolatilitySettings{}.price_tolerance));
    for (const auto convention : {CouponQuoteConvention::shift_maturity_coupon, CouponQuoteConvention::preserve_maturity_coupon}) {
        const auto coupon = implied_coupon(engine, note, context, quote, convention, {.lower_bound = -1.0, .upper_bound = 1.0, .price_tolerance = 1e-11, .max_iterations = 1});
        REQUIRE(coupon);
        CHECK(*coupon == Catch::Approx(0.1).margin(1e-9));
    }
    const auto failed = implied_volatility(engine, note, context, 10.0, {.lower_bound = 0.1, .upper_bound = 0.5});
    REQUIRE_FALSE(failed);
    CHECK(failed.error().category == ErrorCategory::unbracketed_volatility);
    const auto failed_coupon = implied_coupon(engine, note, context, 10.0, CouponQuoteConvention::preserve_maturity_coupon,
                                              {.lower_bound = -0.2, .upper_bound = 0.2});
    REQUIRE_FALSE(failed_coupon);
    CHECK(failed_coupon.error().category == ErrorCategory::unbracketed_coupon);
    const auto invalid = engine.price_with_greeks(note, context, {Greek::delta}, {.spot_shift = 0.0});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
}

TEST_CASE("Quadrature shared engine calls retain independent request state", "[autocallable-quadrature]")
{
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto note = *make_binary_snowball_option({.maturity_coupon_rate = 0.0, .effective_date = start, .expiry_date = end, .observations = {{end, 100.0, 0.1}}});
    const QuadratureAutocallableEngine engine;
    std::array<std::future<double>, 3> prices;
    for (std::size_t i = 0; i < prices.size(); ++i)
        prices[i] = std::async(std::launch::async, [&, i] {
            const double spot = 90.0 + 10.0 * static_cast<double>(i);
            const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), spot, start);
            return engine.price(note, context).value();
        });
    for (std::size_t i = 0; i < prices.size(); ++i)
        CHECK(prices[i].get() == Catch::Approx(binary_reference(90.0 + 10.0 * static_cast<double>(i), 0.3, 0.04, 1.0)).margin(1e-10));
}

TEST_CASE("Autocallable quadrature reused curves match independent repricing", "[autocallable-quadrature][quadrature-reuse]")
{
    const auto start = day(2025, 1, 1), end = day(2025, 1, 20);
    const auto engine = make_quadrature_autocallable_engine({128, 8.0}).value();
    const NumericalShiftSettings shifts{.spot_shift = 0.25, .volatility_shift = 0.002, .rate_shift = 0.001};
    for (const auto history : {AutocallableBarrierState::none, AutocallableBarrierState::knocked_in}) {
        const auto note = make_snowball_option({.maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = history, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 1, 8), 110.0, 0.08}, {end, 105.0, 0.08}}}).value();
        for (const auto valuation : {day(2025, 1, 4), day(2025, 1, 8)}) {
            for (const double spot : {79.9, 80.0, 100.0, 100.25, 109.9}) {
                CAPTURE(history, valuation.time_since_epoch().count(), spot);
                const auto context = make_pricing_context(*make_bsm_parameters(-0.02, 0.01, 0.3), spot,
                                                          valuation, all_days_calendar())
                                         .value();
                const auto expected = test::price_differences(engine, note, context, shifts);
                const auto actual = engine.price_with_greeks(note, context, true, shifts);
                REQUIRE(expected);
                REQUIRE(actual);
                CHECK(actual->price() == expected->price());
                CHECK(actual->values_view() == expected->values_view());
            }
        }
    }
    const auto note = make_binary_snowball_option({.maturity_coupon_rate = 0.06, .effective_date = start, .expiry_date = end, .observations = {{day(2025, 1, 8), 110.0, 0.08}, {end, 105.0, 0.08}}}).value();
    const auto context = make_pricing_context(*make_bsm_parameters(-0.02, 0.01, 0.3), 100.0,
                                              day(2025, 1, 4), all_days_calendar())
                             .value();
    const auto expected = test::price_differences(engine, note, context, shifts);
    const auto actual = engine.price_with_greeks(note, context, true, shifts);
    REQUIRE(expected);
    REQUIRE(actual);
    CHECK(actual->price() == expected->price());
    CHECK(actual->values_view() == expected->values_view());
}
} // namespace
