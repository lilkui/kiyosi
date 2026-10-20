#include <kiyosi/pricing/engines/accumulator/integral.hpp>

#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <future>
#include <limits>
#include <numbers>
#include <numeric>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"
#include "support/numerical.hpp"

namespace {
using namespace kiyosi;
using kiyosi::test::day;

Accumulator contract(Date end, double knock_out = 110.0, double acceleration = 2.0, double daily = 1.0, double quantity = 3.0)
{
    return make_accumulator({100.0, knock_out, daily, acceleration, quantity, day(2025, 1, 1), end}).value();
}

PricingContext market(double spot, Timestamp valuation, double volatility = 0.2, double rate = 0.04,
                      double dividend = 0.01, TradingCalendar calendar = weekdays_calendar())
{
    return make_pricing_context(*make_bsm_parameters(rate, dividend, volatility), spot, valuation, std::move(calendar)).value();
}

// Independent cash/asset digital decomposition of the final observation payoff.
struct TerminalReference {
    Result<double> price(const Accumulator& option, const PricingContext& context) const
    {
        const double time = *year_fraction(context.valuation_time(), start_of_day(option.expiry_date()));
        const auto parameters = context.model_parameters();
        const double deviation = parameters.volatility() * std::sqrt(time);
        const double stock = context.spot_price() * std::exp(-parameters.dividend_yield() * time);
        const double cash = option.strike() * std::exp(-parameters.risk_free_rate() * time);
        const auto lower_payoff = [&](double level) {
            const double d2 = (std::log(context.spot_price() / level) +
                               (parameters.risk_free_rate() - parameters.dividend_yield() -
                                0.5 * parameters.volatility() * parameters.volatility()) *
                                   time) /
                              deviation;
            return 0.5 * (stock * std::erfc((d2 + deviation) / std::numbers::sqrt2) - cash * std::erfc(d2 / std::numbers::sqrt2));
        };
        const double purchase = option.knock_out_level() > option.strike()
                                    ? lower_payoff(option.knock_out_level()) + (option.acceleration_factor() - 1.0) * lower_payoff(option.strike())
                                    : option.acceleration_factor() * lower_payoff(option.knock_out_level());
        return option.accumulated_quantity() * (stock - cash) + option.daily_quantity() * purchase;
    }
};

double exposure(const Accumulator& option, const PricingContext& context)
{
    int observations = 0;
    for (auto date = date_of(context.valuation_time()); date <= option.expiry_date(); date += std::chrono::days{1})
        if (start_of_day(date) >= context.valuation_time() && context.calendar().is_trading_day(date)) ++observations;
    return option.strike() * (option.accumulated_quantity() + observations * option.daily_quantity() * std::max(1.0, option.acceleration_factor()));
}

TEST_CASE("Accumulator quadrature integrates a single observation independently of its spatial budget", "[accumulator-quadrature]")
{
    const auto end = day(2025, 1, 6);
    const auto valuation = start_of_day(day(2025, 1, 3)) + std::chrono::hours{12};
    const auto minimal = make_quadrature_accumulator_engine({4, 0.1}).value();
    for (const double knock_out : {90.0, 100.0, 110.0})
        for (const double acceleration : {0.0, 1.0, 2.0})
            for (const double spot : {80.0, 100.0, 120.0}) {
                CAPTURE(knock_out, acceleration, spot);
                const auto option = contract(end, knock_out, acceleration);
                const auto context = market(spot, valuation, 0.6, -0.02, 0.03);
                const auto actual = QuadratureAccumulatorEngine{}.price(option, context);
                REQUIRE(actual);
                CHECK(*actual == Catch::Approx(*TerminalReference{}.price(option, context)).margin(1e-9));
                CHECK(*minimal.price(option, context) == *actual);
            }
    const auto option = contract(end);
    const auto last_second = market(100.0, start_of_day(end) - std::chrono::seconds{1});
    CHECK(*QuadratureAccumulatorEngine{}.price(option, last_second) == Catch::Approx(*TerminalReference{}.price(option, last_second)).margin(1e-9));
}

TEST_CASE("Accumulator quadrature preserves observation boundaries and validated settlements", "[accumulator-quadrature]")
{
    const auto end = day(2025, 1, 6);
    const auto option = contract(end);
    const QuadratureAccumulatorEngine engine;
    for (const auto [spot, expected] : std::array{std::array{90.0, -50.0}, std::array{100.0, 0.0},
                                                  std::array{109.0, 36.0}, std::array{110.0, 30.0}, std::array{111.0, 33.0}}) {
        CAPTURE(spot);
        const auto context = market(spot, start_of_day(end));
        CHECK(*engine.price(option, context) == expected);
        const auto result = engine.price_with_greeks(option, context, true);
        REQUIRE(result);
        for (const auto& value : result->values_view())
            CHECK_FALSE(value.has_value());
    }
    CHECK(*engine.price(option, market(110.0, start_of_day(day(2025, 1, 1)))) == 30.0);
    const auto outside = engine.price(option, market(100.0, start_of_day(end + std::chrono::days{1})));
    REQUIRE_FALSE(outside);
    CHECK(outside.error().category == ErrorCategory::invalid_time_range);
    const auto weekend = engine.price(contract(day(2025, 1, 5)), market(100.0, start_of_day(day(2025, 1, 3))));
    REQUIRE_FALSE(weekend);
    CHECK(weekend.error().category == ErrorCategory::invalid_date);
    const auto overflow = engine.price(contract(end, 110.0, 2.0, 1.0, std::numeric_limits<double>::max()), market(111.0, start_of_day(end)));
    REQUIRE_FALSE(overflow);
    CHECK(overflow.error().category == ErrorCategory::invalid_result);
    for (const auto settings : {QuadratureSettings{3, 8.0}, QuadratureSettings{512, 0.0},
                                QuadratureSettings{512, std::numeric_limits<double>::infinity()}}) {
        const auto invalid = make_quadrature_accumulator_engine(settings);
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_parameter);
    }
    CHECK(*engine.price(option, market(110.0, start_of_day(end))) == 30.0);
}

TEST_CASE("Accumulator quadrature processes current purchases once and respects adjacent thresholds", "[accumulator-quadrature]")
{
    const auto end = day(2025, 1, 6);
    const auto option = contract(end);
    const QuadratureAccumulatorEngine engine;
    for (const double spot : {90.0, 100.0, 109.0}) {
        const auto context = market(spot, start_of_day(day(2025, 1, 3)));
        const auto updated = option.replace(spot == 90.0 ? 5.0 : 4.0).value();
        CHECK(*engine.price(option, context) == Catch::Approx(*TerminalReference{}.price(updated, context)).margin(1e-9));
    }
    for (const auto [spot, quantity] : std::array{
             std::array{std::nextafter(100.0, 0.0), 5.0}, std::array{std::nextafter(100.0, 110.0), 4.0},
             std::array{std::nextafter(110.0, 0.0), 4.0}, std::array{std::nextafter(110.0, 120.0), 3.0}})
        CHECK(*engine.price(option, market(spot, start_of_day(end))) == quantity * (spot - 100.0));
    CHECK(*engine.price(contract(end, 90), market(90, start_of_day(end))) == -30.0);
    CHECK(*engine.price(contract(end, 90), market(89, start_of_day(end))) == -55.0);
    const auto context = market(100, start_of_day(day(2025, 1, 3)) + std::chrono::hours{12}, 1e308);
    const auto invalid = engine.price(option, context);
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == ErrorCategory::invalid_result);
}

TEST_CASE("Accumulator quadrature defaults converge across representative contracts", "[accumulator-quadrature]")
{
    struct Scenario {
        Date end{};
        double spot{};
        double sigma{};
        double knock_out{};
        double acceleration{};
        double rate{};
        bool daily{};
    };
    const auto month = day(2025, 2, 3), quarter = day(2025, 4, 1), year = day(2026, 1, 1);
    const std::array scenarios{
        Scenario{month, 80, .05, 105, 2, .04, false}, Scenario{month, 100, .2, 110, 2, .04, false},
        Scenario{month, 120, .6, 150, 1, -.02, true}, Scenario{month, 100, .05, 150, 0, .04, false},
        Scenario{quarter, 80, .2, 110, 2, .04, true}, Scenario{quarter, 100, .05, 105, 2, -.02, false},
        Scenario{quarter, 120, .2, 150, 1, .04, false}, Scenario{quarter, 100, .6, 110, 2, .04, true},
        Scenario{year, 80, .05, 105, 2, .04, false}, Scenario{year, 100, .2, 110, 2, .04, false},
        Scenario{year, 120, .6, 150, 2, -.02, true}, Scenario{year, 100, .05, 150, 1, .04, false},
        Scenario{year, 80, .6, 110, 0, .04, false}, Scenario{quarter, 120, .05, 105, 2, .04, false},
        Scenario{month, 80, .6, 150, 2, -.02, true}, Scenario{quarter, 100, .2, 105, 1, .04, false}};
    const QuadratureAccumulatorEngine normal;
    const auto refined = make_quadrature_accumulator_engine({1024, 8.0}).value();
    const auto expanded = make_quadrature_accumulator_engine({1024, 10.0}).value();
    for (const auto& scenario : scenarios) {
        CAPTURE(scenario.spot, scenario.sigma, scenario.knock_out, scenario.acceleration, scenario.daily);
        const auto option = contract(scenario.end, scenario.knock_out, scenario.acceleration);
        const auto context = market(scenario.spot, start_of_day(day(2025, 1, 1)) + std::chrono::hours{12},
                                    scenario.sigma, scenario.rate, .01, scenario.daily ? all_days_calendar() : weekdays_calendar());
        const auto base = normal.price(option, context), fine = refined.price(option, context), wide = expanded.price(option, context);
        REQUIRE(base);
        REQUIRE(fine);
        REQUIRE(wide);
        CHECK(std::abs(*base - *fine) / exposure(option, context) <= 1e-6);
        CHECK(std::abs(*fine - *wide) / exposure(option, context) <= 1e-6);
    }
}

TEST_CASE("Accumulator quadrature agrees with independently refined finite differences", "[accumulator-quadrature]")
{
    const auto end = day(2025, 4, 1);
    const auto option = contract(end);
    const auto context = market(100.25, start_of_day(day(2025, 1, 1)) + std::chrono::hours{12});
    const auto integral = QuadratureAccumulatorEngine{}.price(option, context);
    const auto finite = make_finite_difference_accumulator_engine({3200, 2000, FiniteDifferenceScheme::crank_nicolson, 440.0}).value().price(option, context);
    const auto finer = make_finite_difference_accumulator_engine({6400, 2000, FiniteDifferenceScheme::crank_nicolson, 440.0}).value().price(option, context);
    const auto finest = make_finite_difference_accumulator_engine({6400, 4000, FiniteDifferenceScheme::crank_nicolson, 440.0}).value().price(option, context);
    REQUIRE(integral);
    REQUIRE(finite);
    REQUIRE(finer);
    REQUIRE(finest);
    CHECK(std::abs(*finite - *finer) / exposure(option, context) <= 1e-6);
    CHECK(std::abs(*finer - *finest) / exposure(option, context) <= 1e-6);
    CHECK(std::abs(*integral - *finite) / exposure(option, context) <= 1e-6);
    CHECK(std::abs(*integral - *finest) / exposure(option, context) <= 1e-6);
}

TEST_CASE("Accumulator FD terminal averaging converges against analytic prices and Greeks", "[fd-accumulator-accuracy]")
{
    const auto end = day(2025, 1, 6);
    const auto context = market(100.25, start_of_day(day(2025, 1, 3)) + std::chrono::hours{12});
    for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson})
        for (const double knock_out : {99.7, 100.0, 100.3, 109.75, 110.0, 110.25}) {
            CAPTURE(scheme, knock_out);
            const auto option = contract(end, knock_out);
            const auto reference = test::price_differences(TerminalReference{}, option, context);
            REQUIRE(reference);
            double coarse_error = 0.0;
            for (const int nodes : {400, 800, 1600}) {
                CAPTURE(nodes);
                const auto engine = make_finite_difference_accumulator_engine({nodes, 2000, scheme, 440.0}).value();
                const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                REQUIRE(actual);
                const double error = std::abs(actual->price() - reference->price());
                CHECK(error < 0.01);
                if (nodes == 400) coarse_error = error;
                if (nodes == 1600) {
                    CHECK(error < coarse_error);
                    CHECK(error < 0.001);
                    CHECK(std::abs(test::greek_value(*actual, Greek::delta) - test::greek_value(*reference, Greek::delta)) < 0.002);
                    CHECK(std::abs(test::greek_value(*actual, Greek::gamma) - test::greek_value(*reference, Greek::gamma)) < 0.002);
                }
            }
        }
}

TEST_CASE("Accumulator FD daily event averaging and damping stabilize prices and Greeks", "[fd-accumulator-accuracy]")
{
    const auto context = market(100.25, start_of_day(day(2025, 1, 1)) + std::chrono::hours{12});
    for (const double knock_out : {109.75, 110.0, 110.25}) {
        CAPTURE(knock_out);
        const auto option = contract(day(2025, 4, 1), knock_out);
        const auto reference = make_quadrature_accumulator_engine({1024, 8.0}).value().price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(reference);
        for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler}) {
            CAPTURE(scheme);
            const auto actual = make_finite_difference_accumulator_engine({400, 2000, scheme, 440.0}).value().price(option, context);
            REQUIRE(actual);
            CHECK(std::abs(*actual - reference->price()) / exposure(option, context) < 1e-4);
        }
        const auto coarse = make_finite_difference_accumulator_engine({800, 2000, FiniteDifferenceScheme::crank_nicolson, 440.0}).value().price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        const auto fine = make_finite_difference_accumulator_engine({3200, 2000, FiniteDifferenceScheme::crank_nicolson, 440.0}).value().price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(coarse);
        REQUIRE(fine);
        CHECK(std::abs(coarse->price() - reference->price()) / exposure(option, context) < 2e-5);
        CHECK(std::abs(fine->price() - reference->price()) / exposure(option, context) < 1e-6);
        for (const auto greek : {Greek::delta, Greek::gamma}) {
            const double coarse_error = std::abs(test::greek_value(*coarse, greek) - test::greek_value(*reference, greek));
            const double fine_error = std::abs(test::greek_value(*fine, greek) - test::greek_value(*reference, greek));
            CHECK(fine_error < coarse_error);
            CHECK(fine_error < 0.01);
        }
        const auto short_grid = make_finite_difference_accumulator_engine({1600, 200, FiniteDifferenceScheme::crank_nicolson, 440.0}).value().price_with_greeks(option, context, {Greek::gamma});
        REQUIRE(short_grid);
        CHECK(std::abs(test::greek_value(*short_grid, Greek::gamma) - test::greek_value(*reference, Greek::gamma)) < 0.15);
    }
}

TEST_CASE("Accumulator quadrature lies within Monte Carlo batch sampling uncertainty", "[accumulator-quadrature]")
{
    const auto option = contract(day(2025, 4, 1));
    const auto context = market(100.25, start_of_day(day(2025, 1, 1)) + std::chrono::hours{12});
    std::array<double, 16> batches{};
    for (std::size_t batch = 0; batch < batches.size(); ++batch) {
        const auto value = make_monte_carlo_accumulator_engine({8192, 100 + batch}).value().price(option, context);
        REQUIRE(value);
        batches[batch] = *value;
    }
    const double mean = std::accumulate(batches.begin(), batches.end(), 0.0) / static_cast<double>(batches.size());
    double squares = 0.0;
    for (const double batch : batches)
        squares += (batch - mean) * (batch - mean);
    const double standard_error = std::sqrt(squares / static_cast<double>(batches.size() * (batches.size() - 1)));
    REQUIRE(standard_error > 0.0);
    CHECK(std::abs(*QuadratureAccumulatorEngine{}.price(option, context) - mean) <= 6.0 * standard_error);
}

TEST_CASE("Accumulator quadrature preserves quantity affinity exact limits and shared-instance concurrency", "[accumulator-quadrature]")
{
    const auto end = day(2025, 2, 3);
    const auto context = market(90, start_of_day(day(2025, 1, 1)) + std::chrono::hours{12});
    const QuadratureAccumulatorEngine engine;
    const auto first = engine.price(contract(end, 110, 2, 1, 0), context);
    const auto second = engine.price(contract(end, 110, 2, 1, 3), context);
    const auto third = engine.price(contract(end, 110, 2, 1, 6), context);
    REQUIRE(first);
    REQUIRE(second);
    REQUIRE(third);
    CHECK(*third - *second == Catch::Approx(*second - *first).margin(1e-10));
    CHECK(*engine.price(contract(end, 110, 2, 0, 0), context) == 0.0);
    const auto zero_carry = market(90, context.valuation_time(), .2, 0, 0);
    CHECK(*engine.price(contract(end, 110, 2, 0, 3), zero_carry) == -30.0);
    const auto tiny_vol = market(90, context.valuation_time(), 1e-8, 0, 0);
    CHECK(*engine.price(contract(day(2025, 1, 2), 110, 2, 1, 3), tiny_vol) == Catch::Approx(-50.0).margin(1e-9));
    // Both tiny-volatility paths lie outside the finite grid and exercise the affine tails.
    CHECK(*engine.price(contract(end), tiny_vol) == Catch::Approx(-490.0).margin(1e-9));
    CHECK(*engine.price(contract(end), market(120, context.valuation_time(), 1e-8, 0, 0)) == Catch::Approx(60.0).margin(1e-9));
    auto async = std::async(std::launch::async, [&] { return engine.price(contract(end), context); });
    CHECK(*engine.price(contract(end), context) == *async.get());
}

TEST_CASE("Accumulator quadrature Greek availability follows shared observation rules", "[accumulator-quadrature]")
{
    const auto option = contract(day(2025, 2, 3));
    const QuadratureAccumulatorEngine engine;
    const auto boundary = engine.price_with_greeks(option, market(100, start_of_day(day(2025, 1, 1))), true);
    REQUIRE(boundary);
    CHECK_FALSE(boundary->has(Greek::delta));
    CHECK_FALSE(boundary->has(Greek::gamma));
    CHECK_FALSE(boundary->has(Greek::speed));
    CHECK_FALSE(boundary->has(Greek::theta));
    const auto small_vol = engine.price_with_greeks(option, market(90, start_of_day(day(2025, 1, 3)) + std::chrono::hours{12}, 1e-4), {Greek::vega, Greek::vanna, Greek::zomma});
    REQUIRE(small_vol);
    CHECK_FALSE(small_vol->has(Greek::vega));
    CHECK_FALSE(small_vol->has(Greek::vanna));
    CHECK_FALSE(small_vol->has(Greek::zomma));
}

TEST_CASE("Accumulator quadrature Greeks meet normalized grid and bump convergence targets", "[accumulator-quadrature]")
{
    struct Tolerance {
        Greek greek;
        int spot_order;
        double absolute;
        double relative;
    };
    const std::array tolerances{
        Tolerance{Greek::delta, 1, 1e-6, .005}, Tolerance{Greek::gamma, 2, 1e-5, .01},
        Tolerance{Greek::vega, 0, 1e-8, .005}, Tolerance{Greek::rho, 0, 1e-8, .005},
        Tolerance{Greek::theta, 0, 1e-8, .02}, Tolerance{Greek::speed, 3, 1e-4, .05},
        Tolerance{Greek::vanna, 1, 1e-7, .05}, Tolerance{Greek::charm, 1, 1e-7, .05},
        Tolerance{Greek::zomma, 2, 1e-6, .05}, Tolerance{Greek::color, 2, 1e-6, .05}};
    const auto refined = make_quadrature_accumulator_engine({1024, 8.0}).value();
    const auto check = [&](const Accumulator& option, const PricingContext& context) {
        CAPTURE(context.spot_price(), context.model_parameters().volatility(), option.expiry_date().time_since_epoch().count());
        const auto base = QuadratureAccumulatorEngine{}.price_with_greeks(option, context, true);
        const auto fine = refined.price_with_greeks(option, context, true);
        const auto smaller = refined.price_with_greeks(option, context, true, {.spot_shift = .005, .volatility_shift = .00005, .rate_shift = .00005});
        REQUIRE(base);
        REQUIRE(fine);
        REQUIRE(smaller);
        for (const auto& tolerance : tolerances) {
            CAPTURE(tolerance.greek);
            REQUIRE(base->has(tolerance.greek));
            REQUIRE(fine->has(tolerance.greek));
            REQUIRE(smaller->has(tolerance.greek));
            const double scale = std::pow(option.strike(), tolerance.spot_order) / exposure(option, context);
            const double reference = test::greek_value(*fine, tolerance.greek) * scale;
            const double limit = tolerance.absolute + tolerance.relative * std::abs(reference);
            CHECK(std::abs(test::greek_value(*base, tolerance.greek) * scale - reference) <= limit);
            CHECK(std::abs(test::greek_value(*smaller, tolerance.greek) * scale - reference) <= limit);
        }
    };
    const auto valuation = start_of_day(day(2025, 1, 4)) + std::chrono::hours{12};
    check(contract(day(2025, 2, 3)), market(100.25, valuation));
    check(contract(day(2026, 1, 1), 105), market(80, valuation, .05));
    check(contract(day(2025, 4, 1), 150, 1), market(120, valuation, .6, -.02));
}

TEST_CASE("Accumulator quadrature single-event Greeks agree with independent terminal prices", "[accumulator-quadrature]")
{
    const auto option = contract(day(2025, 1, 6));
    const auto context = market(101.5, start_of_day(day(2025, 1, 4)) + std::chrono::hours{12}, .6);
    const NumericalShiftSettings shifts{.spot_shift = .2, .volatility_shift = .001, .rate_shift = .001};
    const auto actual = QuadratureAccumulatorEngine{}.price_with_greeks(option, context, true, shifts);
    const auto reference = test::price_differences(TerminalReference{}, option, context, shifts);
    REQUIRE(actual);
    REQUIRE(reference);
    for (std::size_t index = 0; index < greek_count; ++index) {
        const auto greek = static_cast<Greek>(index);
        CAPTURE(greek);
        REQUIRE(actual->has(greek));
        REQUIRE(reference->has(greek));
        const double expected = test::greek_value(*reference, greek);
        CHECK(test::greek_value(*actual, greek) == Catch::Approx(expected).margin(1e-7 + 1e-5 * std::abs(expected)));
    }
    const auto quote = QuadratureAccumulatorEngine{}.price(option, context);
    REQUIRE(quote);
    const auto volatility = implied_volatility(QuadratureAccumulatorEngine{}, option, context, *quote,
                                               {.lower_bound = .4, .upper_bound = .8});
    REQUIRE(volatility);
    CHECK(*volatility == Catch::Approx(.6).margin(1e-6));
}

} // namespace
