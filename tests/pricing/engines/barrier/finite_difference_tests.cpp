#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <optional>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "pricing/detail/math.hpp"
#include "support/common.hpp"

TEST_CASE("Continuous up-and-out grids retain explicit stability validation", "[fd-barrier-stability]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.1), 100.0, start);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out});
    const auto stable = make_finite_difference_barrier_engine({200, 50, FiniteDifferenceScheme::explicit_euler}).value().price(option, context);
    REQUIRE(stable);
    const auto result = make_finite_difference_barrier_engine({200, 1, FiniteDifferenceScheme::explicit_euler}).value().price(option, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == ErrorCategory::invalid_parameter);
}

TEST_CASE("Continuous single barriers retain exact levels near knock-out", "[fd-barrier-boundary]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto parameters = *make_bsm_parameters(0.04, 0.0, 0.2);
    for (const double barrier : {120.0, 120.13, 80.0, 79.87}) {
        const bool up = barrier > 100.0;
        for (const bool knock_in : {false, true})
            for (const double rebate : {0.0, 2.0}) {
                const auto option = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put,
                                                          .strike = 100.0,
                                                          .effective_date = start,
                                                          .expiry_date = end,
                                                          .barrier_level = barrier,
                                                          .barrier_type = up ? (knock_in ? BarrierType::up_and_in : BarrierType::up_and_out)
                                                                             : (knock_in ? BarrierType::down_and_in : BarrierType::down_and_out),
                                                          .rebate = rebate});
                for (const double distance : {0.01, 1e-6, 0.0}) {
                    CAPTURE(barrier, knock_in, rebate, distance);
                    const auto context = *make_pricing_context(parameters, barrier + (up ? -distance : distance), start);
                    const auto expected = AnalyticBarrierEngine{}.price(option, context);
                    const auto actual = FiniteDifferenceBarrierEngine{}.price(option, context);
                    REQUIRE(expected);
                    REQUIRE(actual);
                    CHECK_THAT(*actual, Catch::Matchers::WithinAbs(*expected, 1e-9 + 0.005 * std::abs(*expected)));
                }
            }
    }
}

TEST_CASE("Scheduled barrier peaks preserve prices and Greeks", "[fd-extrema]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}});
    const auto lower = *make_european_option(OptionType::call, 100.0, start, end);
    const auto upper = *make_european_option(OptionType::call, 120.0, start, end);
    const auto cash = *make_cash_or_nothing_option(OptionType::call, 120.0, 20.0, start, end);
    const auto engine = make_finite_difference_barrier_engine({.asset_upper_boundary = 400.0}).value();
    const auto parameters = *make_bsm_parameters(0.04, 0.01, 0.2);
    const auto market = [&](double spot) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext owns its calendar and is returned by value.
        return make_pricing_context(parameters, spot, start, all_days_calendar()).value();
    };
    for (const double spot : {111.94, 111.97, 112.01}) {
        CAPTURE(spot);
        const auto context = market(spot);
        const GreeksRequest request{Greek::delta, Greek::gamma};
        const auto actual = engine.price_with_greeks(option, context, request);
        const auto first = AnalyticVanillaEngine{}.price_with_greeks(lower, context, request);
        const auto second = AnalyticVanillaEngine{}.price_with_greeks(upper, context, request);
        const auto digital = AnalyticDigitalEngine{}.price_with_greeks(cash, context, request);
        REQUIRE(actual);
        REQUIRE(first);
        REQUIRE(second);
        REQUIRE(digital);
        CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(first->price() - second->price() - digital->price(), 0.001));
        for (const auto greek : {Greek::delta, Greek::gamma}) {
            const double expected = test::greek_value(*first, greek) - test::greek_value(*second, greek) - test::greek_value(*digital, greek);
            CHECK_THAT(test::greek_value(*actual, greek), Catch::Matchers::WithinAbs(expected, greek == Greek::delta ? 5e-5 : 1e-5));
        }
        constexpr double bump = 0.01;
        const auto high = engine.price(option, market(spot + bump));
        const auto low = engine.price(option, market(spot - bump));
        REQUIRE(high);
        REQUIRE(low);
        CHECK_THAT(test::greek_value(*actual, Greek::delta), Catch::Matchers::WithinAbs((*high - *low) / (2.0 * bump), 1e-7));
        CHECK_THAT(test::greek_value(*actual, Greek::gamma), Catch::Matchers::WithinAbs((*high - 2.0 * actual->price() + *low) / (bump * bump), 1e-6));
    }
}

TEST_CASE("Scheduled barrier edges retain the next observation settlement", "[fd-barrier-edges]")
{
    using namespace kiyosi;
    const auto scheme = GENERATE(FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson);
    const bool up = GENERATE(false, true);
    const auto timing = GENERATE(RebateTiming::at_hit, RebateTiming::at_expiry);
    const double rebate = GENERATE(0.0, 2.0);
    const bool observes_expiry = GENERATE(false, true);
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto valuation = start_of_day(start) + std::chrono::hours{12};
    std::vector<Date> dates{start + std::chrono::days{1}};
    if (observes_expiry) dates.push_back(end);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), up ? 150.0 : 1.0, valuation, all_days_calendar());
    const auto option = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 110.0 : 90.0, .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out, .rebate = rebate, .rebate_timing = timing, .observation_mode = ObservationMode::scheduled, .observation_dates = dates});
    const auto engine = *make_finite_difference_barrier_engine({200, scheme == FiniteDifferenceScheme::explicit_euler ? 2'000 : 200, scheme, up ? std::optional{151.0} : std::nullopt});
    const auto price = engine.price(option, context);
    REQUIRE(price);
    const auto payment_date = timing == RebateTiming::at_hit ? dates.front() : end;
    const double expected = rebate * std::exp(-0.04 * *year_fraction(valuation, start_of_day(payment_date)));
    CHECK_THAT(*price, Catch::Matchers::WithinAbs(expected, 1e-6));
}

TEST_CASE("Scheduled terminal barrier projection converges across grid placements", "[fd-barrier-events]")
{
    using namespace kiyosi;
    const bool up = GENERATE(false, true);
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto type = up ? OptionType::call : OptionType::put;
    const double sign = up ? 1.0 : -1.0;
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start, all_days_calendar());
    const auto vanilla = AnalyticVanillaEngine{}.price_with_greeks(*make_european_option(type, 100.0, start, end), context, {Greek::delta, Greek::gamma});
    REQUIRE(vanilla);
    std::array previous{1.0, 1.0, 1.0};
    for (const int count : {200, 400, 800}) {
        const auto engine = *make_finite_difference_barrier_engine({count, 2000, FiniteDifferenceScheme::crank_nicolson, 400.0});
        std::array<double, 3> worst{};
        for (const double offset : {-0.25, 0.0, 0.125, 0.25, 0.5}) {
            const double barrier = (up ? 110.0 : 90.0) + offset;
            CAPTURE(up, count, barrier);
            const auto cash = AnalyticDigitalEngine{}.price_with_greeks(*make_cash_or_nothing_option(type, barrier, 1.0, start, end), context, {Greek::delta, Greek::gamma});
            const auto asset = AnalyticDigitalEngine{}.price_with_greeks(*make_asset_or_nothing_option(type, barrier, start, end), context, {Greek::delta, Greek::gamma});
            REQUIRE(cash);
            REQUIRE(asset);
            const auto option = *make_barrier_option({.option_type = type, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = barrier, .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out, .rebate = 2.0, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}});
            const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(actual);
            const std::array expected{
                vanilla->price() - sign * (asset->price() - 100.0 * cash->price()) + 2.0 * cash->price(),
                test::greek_value(*vanilla, Greek::delta) - sign * (test::greek_value(*asset, Greek::delta) - 100.0 * test::greek_value(*cash, Greek::delta)) + 2.0 * test::greek_value(*cash, Greek::delta),
                test::greek_value(*vanilla, Greek::gamma) - sign * (test::greek_value(*asset, Greek::gamma) - 100.0 * test::greek_value(*cash, Greek::gamma)) + 2.0 * test::greek_value(*cash, Greek::gamma)};
            const std::array values{actual->price(), test::greek_value(*actual, Greek::delta), test::greek_value(*actual, Greek::gamma)};
            for (std::size_t index = 0; index < worst.size(); ++index)
                worst[index] = std::max(worst[index], std::abs(values[index] - expected[index]));
            CHECK(*engine.price(option, context) == actual->price());
        }
        const double refinement = 200.0 / count;
        CHECK(worst[0] < 0.012 * refinement * refinement);
        CHECK(worst[1] < 0.00013 * refinement);
        CHECK(worst[2] < 0.00004 * refinement * refinement);
        for (std::size_t index = 0; index < worst.size(); ++index)
            CHECK(worst[index] < previous[index]);
        previous = worst;
    }
}

TEST_CASE("Scheduled barrier projection preserves large finite rebates", "[fd-barrier-events]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start, all_days_calendar());
    const auto engine = *make_finite_difference_barrier_engine({40, 2000, FiniteDifferenceScheme::implicit_euler, 400.0});
    const auto price_at_rebate = [&](double rebate) {
        const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 110.0, .barrier_type = BarrierType::up_and_out, .rebate = rebate, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}});
        return engine.price(option, context);
    };
    const auto base = price_at_rebate(0.0);
    const auto unit = price_at_rebate(1.0);
    const auto large = price_at_rebate(1e308);
    REQUIRE(base);
    REQUIRE(unit);
    REQUIRE(large);
    CHECK_THAT(*large / 1e308, Catch::Matchers::WithinAbs(*unit - *base, 1e-12));
}

TEST_CASE("Scheduled barrier restarts match implicit half-steps without extra observations", "[fd-barrier-events]")
{
    using namespace kiyosi;
    const bool up = GENERATE(false, true);
    const auto timing = GENERATE(RebateTiming::at_hit, RebateTiming::at_expiry);
    const auto start = test::day(2025, 1, 1), end = start + std::chrono::days{360};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start, all_days_calendar());
    const auto option = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 110.125 : 90.125, .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out, .rebate = 2.0, .rebate_timing = timing, .observation_mode = ObservationMode::scheduled, .observation_dates = {start + std::chrono::days{90}, start + std::chrono::days{180}, start + std::chrono::days{270}, end}});
    const auto cn = *make_finite_difference_barrier_engine({200, 4, FiniteDifferenceScheme::crank_nicolson, 400.0});
    const auto implicit = *make_finite_difference_barrier_engine({200, 8, FiniteDifferenceScheme::implicit_euler, 400.0});
    const auto actual = cn.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
    const auto expected = implicit.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
    REQUIRE(actual);
    REQUIRE(expected);
    CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(expected->price(), 1e-12));
    for (const auto greek : {Greek::delta, Greek::gamma})
        CHECK_THAT(test::greek_value(*actual, greek), Catch::Matchers::WithinAbs(test::greek_value(*expected, greek), 1e-12));
}

TEST_CASE("Scheduled barrier events match independent two-observation integration", "[fd-barrier-events]")
{
    using namespace kiyosi;
    const bool up = GENERATE(false, true);
    const auto timing = GENERATE(RebateTiming::at_hit, RebateTiming::at_expiry);
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start, all_days_calendar());
    const auto option = *make_barrier_option({.option_type = up ? OptionType::call : OptionType::put, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 110.125 : 90.125, .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out, .rebate = 2.0, .rebate_timing = timing, .observation_mode = ObservationMode::scheduled, .observation_dates = {start + std::chrono::days{180}, end}});
    // Conditional analytic terminal payoffs integrated against the first fixing's normal density.
    // Python's matching regression checks Simpson refinement and the independent Greek stencil.
    const std::array expected = up
                                    ? (timing == RebateTiming::at_hit ? std::array{1.4554158324974538, 0.040123419859492415, -0.0018575837515300004} : std::array{1.4450612046575544, 0.03921123536516191, -0.0018895460854650987})
                                    : (timing == RebateTiming::at_hit ? std::array{1.3756950644643724, -0.04356103943840228, -0.000799252307714937} : std::array{1.3672793398072463, -0.04274689847083124, -0.000853236609543728});
    for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson}) {
        CAPTURE(up, timing, scheme);
        const int count = scheme == FiniteDifferenceScheme::explicit_euler ? 200 : 800;
        const auto engine = *make_finite_difference_barrier_engine({count, 2000, scheme, 400.0});
        const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(actual);
        const double scale = 800.0 / count;
        CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(expected[0], 0.0008 * scale * scale));
        CHECK_THAT(test::greek_value(*actual, Greek::delta), Catch::Matchers::WithinAbs(expected[1], 0.00003 * scale * scale));
        CHECK_THAT(test::greek_value(*actual, Greek::gamma), Catch::Matchers::WithinAbs(expected[2], 0.000003 * scale * scale));
        if (scheme == FiniteDifferenceScheme::crank_nicolson) {
            const std::array fine{actual->price(), test::greek_value(*actual, Greek::delta), test::greek_value(*actual, Greek::gamma)};
            std::array previous{1.0, 1.0, 1.0};
            for (const int steps : {16, 64, 256}) {
                CAPTURE(steps);
                const auto refined = make_finite_difference_barrier_engine({800, steps, scheme, 400.0})->price_with_greeks(option, context, {Greek::delta, Greek::gamma});
                REQUIRE(refined);
                const std::array values{refined->price(), test::greek_value(*refined, Greek::delta), test::greek_value(*refined, Greek::gamma)};
                for (std::size_t index = 0; index < previous.size(); ++index) {
                    const double error = std::abs(values[index] - fine[index]);
                    CHECK(error < previous[index]);
                    previous[index] = error;
                }
            }
            CHECK(previous[0] < 2e-6);
            CHECK(previous[1] < 5e-7);
            CHECK(previous[2] < 3e-8);
        }
    }
}

TEST_CASE("Scheduled current barrier observations use the actual spot before projection", "[fd-barrier-events]")
{
    using namespace kiyosi;
    const bool up = GENERATE(false, true);
    const bool knock_in = GENERATE(false, true);
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const double barrier = up ? 110.125 : 90.125;
    const auto kind = up ? (knock_in ? BarrierType::up_and_in : BarrierType::up_and_out) : (knock_in ? BarrierType::down_and_in : BarrierType::down_and_out);
    BarrierOptionTerms terms{.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = barrier, .barrier_type = kind, .rebate = 2.0, .observation_mode = ObservationMode::scheduled, .observation_dates = {end}};
    const auto future = *make_barrier_option(terms);
    terms.observation_dates.insert(terms.observation_dates.begin(), start);
    const auto current = *make_barrier_option(terms);
    const auto engine = *make_finite_difference_barrier_engine({200, 2000, FiniteDifferenceScheme::crank_nicolson, 400.0});
    for (const double spot : {std::nextafter(barrier, up ? 0.0 : std::numeric_limits<double>::infinity()), barrier, std::nextafter(barrier, up ? std::numeric_limits<double>::infinity() : 0.0)}) {
        CAPTURE(up, knock_in, spot);
        const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), spot, start, all_days_calendar());
        const bool touched = up ? spot >= barrier : spot <= barrier;
        const auto actual = engine.price_with_greeks(current, context, {Greek::delta, Greek::gamma});
        REQUIRE(actual);
        if (touched) {
            const double expected = knock_in ? *AnalyticVanillaEngine{}.price(*make_european_option(OptionType::call, 100.0, start, end), context) : 2.0 * std::exp(-0.04);
            CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(expected, 1e-12));
        } else {
            const auto expected = engine.price_with_greeks(future, context, {Greek::delta, Greek::gamma});
            REQUIRE(expected);
            CHECK(actual->price() == expected->price());
            for (const auto greek : {Greek::delta, Greek::gamma})
                CHECK_FALSE(actual->has(greek));
        }
    }
}

TEST_CASE("Scheduled barrier projection preserves vanishing-volatility settlement and rebate timing", "[fd-barrier-events]")
{
    using namespace kiyosi;
    const bool up = GENERATE(false, true);
    const bool hit = GENERATE(false, true);
    const auto timing = GENERATE(RebateTiming::at_hit, RebateTiming::at_expiry);
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto first = start + std::chrono::days{180};
    const auto type = up ? OptionType::put : OptionType::call;
    const double spot = (up == hit) ? 120.0 : 80.0;
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.04, 1e-12), spot, start, all_days_calendar());
    const auto option = *make_barrier_option({.option_type = type, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = up ? 110.125 : 90.125, .barrier_type = up ? BarrierType::up_and_out : BarrierType::down_and_out, .rebate = 2.0, .rebate_timing = timing, .observation_mode = ObservationMode::scheduled, .observation_dates = {first, end}});
    const double expected = hit ? 2.0 * std::exp(-0.04 * (timing == RebateTiming::at_hit ? 180.0 / 365.0 : 1.0)) : 20.0 * std::exp(-0.04);
    for (const auto scheme : {FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson}) {
        CAPTURE(up, hit, timing, scheme);
        const auto engine = *make_finite_difference_barrier_engine({800, 2000, scheme, 400.0});
        const auto actual = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
        REQUIRE(actual);
        CHECK_THAT(actual->price(), Catch::Matchers::WithinAbs(expected, 1e-5));
        CHECK_THAT(test::greek_value(*actual, Greek::delta), Catch::Matchers::WithinAbs(hit ? 0.0 : (up ? -1.0 : 1.0) * std::exp(-0.04), 1e-6));
        CHECK_THAT(test::greek_value(*actual, Greek::gamma), Catch::Matchers::WithinAbs(0.0, 2e-10));
    }
}
