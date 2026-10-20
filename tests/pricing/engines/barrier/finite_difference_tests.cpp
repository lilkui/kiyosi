#include <catch2/catch_approx.hpp>
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

#include "pricing/detail/fd_grid.hpp"
#include "pricing/detail/fd_scheme.hpp"
#include "pricing/detail/math.hpp"
#include "support/common.hpp"

TEST_CASE("Continuous up-and-out rollback preserves full-grid prices and Greeks", "[fd-barrier-trim]")
{
    using namespace kiyosi;
    using namespace kiyosi::detail;
    const auto scheme = GENERATE(FiniteDifferenceScheme::explicit_euler, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::crank_nicolson);
    const auto type = GENERATE(OptionType::call, OptionType::put);
    const auto timing = GENERATE(RebateTiming::at_hit, RebateTiming::at_expiry);
    const int steps = GENERATE(3, 4, 31, 200);
    const double barrier = GENERATE(120.0, 121.1);
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 119.25, start);
    const auto option = *make_barrier_option({.option_type = type, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = barrier, .barrier_type = BarrierType::up_and_out, .rebate = 1.5, .rebate_timing = timing});
    const FiniteDifferenceSettings settings{steps, 2'000, scheme, 480.0};
    const auto space = *make_spatial_grid(settings, 480.0, {119.25, 100.0, barrier});
    std::vector<double> full(space.size());
    for (int index = 0; index <= steps; ++index) {
        const double asset = space.spacing * index;
        full[index] = asset >= barrier ? option.rebate()
                                       : std::max(type == OptionType::call ? asset - 100.0 : 100.0 - asset, 0.0);
    }
    const auto rebate = [&](double tau) {
        return timing == RebateTiming::at_hit ? option.rebate() : scaled_exponential(option.rebate(), -0.04 * tau);
    };
    // Keep the original full solve as the reference for pruning fixed rows.
    const auto marched = march_backward<true>(
        make_finite_difference_time_grid(1.0, settings.time_step_count),
        DiffusionParameters{0.04, 0.01, 0.2, scheme_theta(scheme)}, full,
        [&](double tau) { return Boundaries{type == OptionType::put ? 100.0 * std::exp(-0.04 * tau) : 0.0, rebate(tau)}; },
        [](std::vector<double>&, double, double) {},
        [&](int index, double tau) -> std::optional<double> {
            return space.spacing * index >= barrier ? std::optional{rebate(tau)} : std::nullopt;
        });
    REQUIRE(marched);
    const auto engine = make_finite_difference_barrier_engine(settings).value();
    const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma, Greek::speed});
    REQUIRE(result);
    CHECK(result->price() == Catch::Approx(space.interpolate(full, context.spot_price())).epsilon(1e-12));
    CHECK(test::greek_value(*result, Greek::delta) == Catch::Approx(space.delta(full, context.spot_price())).epsilon(1e-12).margin(1e-14));
    CHECK(test::greek_value(*result, Greek::gamma) == Catch::Approx(space.gamma(full, context.spot_price())).epsilon(1e-12).margin(1e-14));
    const double h = symmetric_shift(context.spot_price(), NumericalShiftSettings{}.spot_shift);
    const double speed = (space.gamma(full, context.spot_price() + h) - space.gamma(full, context.spot_price() - h)) / (2.0 * h);
    CHECK(test::greek_value(*result, Greek::speed) == Catch::Approx(speed).epsilon(1e-12).margin(1e-14));
}

TEST_CASE("Continuous up-and-out trimming retains explicit stability validation", "[fd-barrier-trim]")
{
    using namespace kiyosi;
    const auto start = test::day(2025, 1, 1), end = test::day(2026, 1, 1);
    const auto context = *make_pricing_context(*make_bsm_parameters(0.0, 0.0, 0.1), 100.0, start);
    const auto option = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 120.0, .barrier_type = BarrierType::up_and_out});
    const auto result = make_finite_difference_barrier_engine({200, 50, FiniteDifferenceScheme::explicit_euler}).value().price(option, context);
    REQUIRE_FALSE(result);
    CHECK(result.error().category == ErrorCategory::invalid_parameter);
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
        CHECK_THAT(test::greek_value(*actual, Greek::gamma), Catch::Matchers::WithinAbs(0.0, 1e-10));
    }
}
