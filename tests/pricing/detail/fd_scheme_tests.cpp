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

#include "pricing/detail/fd_scheme.hpp"
#include "pricing/detail/fd_grid.hpp"

TEST_CASE("Finite-difference speed validates shifted prices before using gamma", "[fd-speed]")
{
    using namespace kiyosi;
    std::vector<double> prices;
    std::vector<double> gammas;
    bool reject_low = false;
    const auto price_at = [&](double spot) -> Result<double> {
        prices.push_back(spot);
        if (reject_low && spot < 100.0)
            return std::unexpected(Error{ErrorCategory::invalid_result, "shifted price rejected"});
        return spot;
    };
    const auto gamma_at = [&](double spot) {
        gammas.push_back(spot);
        return 3.0 * spot;
    };
    SECTION("Available stencil uses both shifted gammas")
    {
        const auto speed = detail::finite_difference_speed(100.0, 1.0, 200.0, price_at, gamma_at);
        REQUIRE(speed);
        CHECK(*speed == std::optional{3.0});
        CHECK(prices == std::vector<double>{101.0, 99.0});
        CHECK(gammas.size() == 2);
    }
    SECTION("Unavailable stencils perform no shifted valuations")
    {
        for (const auto [spot, h, upper] : {std::array{100.0, 0.0, 200.0},
                                            std::array{1.0, 1.0, 200.0},
                                            std::array{100.0, 1.0, 101.0},
                                            std::array{100.0, std::numeric_limits<double>::quiet_NaN(), 200.0}}) {
            const auto speed = detail::finite_difference_speed(spot, h, upper, price_at, gamma_at);
            REQUIRE(speed);
            CHECK_FALSE(speed->has_value());
        }
        CHECK(prices.empty());
        CHECK(gammas.empty());
    }
    SECTION("A failed feasible price prevents gamma evaluation")
    {
        reject_low = true;
        const auto speed = detail::finite_difference_speed(100.0, 1.0, 200.0, price_at, gamma_at);
        REQUIRE_FALSE(speed);
        CHECK(speed.error().category == ErrorCategory::invalid_result);
        CHECK(prices == std::vector<double>{101.0, 99.0});
        CHECK(gammas.empty());
    }
}

TEST_CASE("Finite-difference spline interpolation preserves nodes and affine values")
{
    for (const int steps : {3, 8}) {
        const kiyosi::detail::SpatialGrid grid{static_cast<double>(steps), 1.0, steps};
        std::vector<double> values(grid.size());
        for (std::size_t index = 0; index < values.size(); ++index)
            values[index] = 2.0 * static_cast<double>(index) - 3.0;
        for (int index = 0; index <= 4 * steps; ++index) {
            const double spot = static_cast<double>(index) / 4.0;
            CHECK(std::abs(grid.interpolate(values, spot) - (2.0 * spot - 3.0)) < 1e-12);
        }
        values[1] += 0.5;
        for (int index = 0; index <= steps; ++index)
            CHECK(grid.interpolate(values, static_cast<double>(index)) == values[index]);
    }
}

TEST_CASE("Finite-difference interpolation returns exact nodes without snapping nearby spots", "[fd-performance]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4};
    const double large = std::numeric_limits<double>::max();
    const std::array extreme{large, -large, large, -large, large};
    for (std::size_t index = 0; index < extreme.size(); ++index)
        CHECK(grid.interpolate(extreme, static_cast<double>(index)) == extreme[index]);

    const std::array affine{-3.0, -1.0, 1.0, 3.0, 5.0};
    for (const double spot : {std::nextafter(1.0, 0.0), std::nextafter(1.0, 2.0)}) {
        CHECK(grid.interpolate(affine, spot) == 2.0 * spot - 3.0);
        CHECK(grid.interpolate(affine, spot) != affine[1]);
    }
    const std::array curved{0.0, 1.0, 0.0, 1.0, 0.0};
    CHECK(std::abs(grid.interpolate(curved, 0.5) - 43.0 / 56.0) < 1e-15);
    CHECK(std::abs(grid.interpolate(curved, 1.5) - 25.0 / 56.0) < 1e-15);
}

TEST_CASE("Finite-difference spline curvature supports repeated lookups", "[fd-performance]")
{
    const kiyosi::detail::SpatialGrid grid{4.0, 1.0, 4};
    const std::array curved{0.0, 1.0, 0.0, 1.0, 0.0};
    std::vector<double> curvature;
    CHECK(grid.interpolate(curved, 1.0, curvature) == 1.0);
    CHECK(curvature.empty());
    for (const double spot : {0.5, 1.5, 3.5, 0.5}) {
        CAPTURE(spot);
        const double expected = spot == 1.5 ? 25.0 / 56.0 : 43.0 / 56.0;
        CHECK(std::abs(grid.interpolate(curved, spot, curvature) - expected) < 1e-15);
    }
    for (std::size_t index = 0; index < curved.size(); ++index)
        CHECK(grid.interpolate(curved, static_cast<double>(index), curvature) == curved[index]);
}

TEST_CASE("Finite-difference spline interpolation stays within neighboring payoff values", "[audit-fixes]")
{
    const kiyosi::detail::SpatialGrid grid{8.0, 1.0, 8};
    for (const double sign : {1.0, -1.0}) {
        std::vector<double> values(grid.size());
        for (std::size_t index = 0; index < values.size(); ++index)
            values[index] = std::max(sign * (static_cast<double>(index) - 4.0), 0.0);
        std::vector<double> curvature;
        for (int sample = 0; sample <= 32; ++sample) {
            const double spot = static_cast<double>(sample) / 4.0;
            const auto index = std::min(static_cast<std::size_t>(spot), values.size() - 2);
            const double value = grid.interpolate(values, spot);
            CHECK(value >= std::min(values[index], values[index + 1]));
            CHECK(value <= std::max(values[index], values[index + 1]));
            CHECK(grid.interpolate(values, spot, curvature) == value);
        }
        CHECK(grid.interpolate(values, sign > 0.0 ? 3.75 : 4.25) == 0.0);
    }
}

TEST_CASE("Linear boundary steppers refresh parameters without retaining old operators", "[fd-implied-workspace]")
{
    using namespace kiyosi::detail;
    const std::vector<double> first{1.0, 1.2, 1.5, 2.1, 3.0}, second{2.0, 1.5, 1.0, 0.5, 0.0};
    std::vector<double> actual(first.size()), expected(first.size()), actual_second(first.size()), expected_second(first.size());
    LinearBoundaryStepper reused(first.size(), 4.0, 1.0, {0.04, 0.01, 0.2, 0.5});
    for (const auto parameters : {DiffusionParameters{0.04, 0.01, 0.2, 0.5}, {0.1, -0.02, 0.7, 1.0}, {0.04, 0.01, 0.2, 0.5}}) {
        reused.set_parameters(parameters);
        LinearBoundaryStepper fresh(first.size(), 4.0, 1.0, parameters);
        REQUIRE(reused.advance(first, actual, 0.01));
        REQUIRE(fresh.advance(first, expected, 0.01));
        CHECK(actual == expected);
        REQUIRE(reused.advance_pair(first, actual, second, actual_second, 0.01));
        REQUIRE(fresh.advance_pair(first, expected, second, expected_second, 0.01));
        CHECK(actual == expected);
        CHECK(actual_second == expected_second);
    }
}

TEST_CASE("Finite-difference grids reject zero spacing while preserving small positive spacing", "[audit-fixes]")
{
    const double tiny = std::numeric_limits<double>::denorm_min();
    const auto invalid = kiyosi::detail::make_spatial_grid({}, 4.0 * tiny, {tiny});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
    for (const double upper : {400.0 * tiny, 400.0}) {
        const auto valid = kiyosi::detail::make_spatial_grid({}, upper, {tiny});
        REQUIRE(valid);
        CHECK(valid->spacing == upper / 200.0);
        CHECK(valid->spacing > 0.0);
        CHECK(std::isfinite(valid->spacing));
    }
}

TEST_CASE("Finite-difference grids preserve exact expiry_date without replaying terminal events", "[cross-validation]")
{
    const double maturity = 91.0 / 365.0;
    for (const int steps : {400, 800, 1600}) {
        CAPTURE(steps);
        const auto grid = kiyosi::detail::make_finite_difference_time_grid(maturity, steps, {0.0, maturity});
        CHECK(grid.times.front() == 0.0);
        CHECK(grid.times.back() == maturity);
        CHECK(grid.times.size() == static_cast<std::size_t>(steps + 1));
    }
}

TEST_CASE("Uniform finite-difference intervals share one exact width", "[fd-performance]")
{
    for (const double maturity : {1.0 / (365.0 * 86400.0 * 1e6), 91.0 / 365.0, 1.0, 2.75}) {
        for (const int steps : {1, 3, 80, 200, 1000, 100'000}) {
            CAPTURE(maturity, steps);
            const auto grid = kiyosi::detail::make_finite_difference_time_grid(maturity, steps);
            REQUIRE(grid.time_steps.size() == static_cast<std::size_t>(steps));
            CHECK(std::ranges::all_of(grid.time_steps, [=](double dt) { return dt == maturity / steps; }));
            CHECK(grid.times.front() == 0.0);
            CHECK(grid.times.back() == maturity);
            const auto event_grid = kiyosi::detail::make_finite_difference_time_grid(maturity, steps, {0.0, maturity});
            CHECK(grid.times == event_grid.times);
            CHECK(grid.time_steps == event_grid.time_steps);
        }
    }
}

TEST_CASE("Degenerate finite-difference time grids retain unique nodes", "[fd-performance]")
{
    for (const double maturity : {0.0, 4.0 * std::numeric_limits<double>::denorm_min()}) {
        const auto grid = kiyosi::detail::make_finite_difference_time_grid(maturity, 8);
        const auto event_grid = kiyosi::detail::make_finite_difference_time_grid(maturity, 8, {0.0, maturity});
        CHECK(grid.times == event_grid.times);
        CHECK(grid.time_steps == event_grid.time_steps);
        CHECK(std::ranges::adjacent_find(grid.times) == grid.times.end());
    }
}

TEST_CASE("Finite-difference event intervals retain exact anchors and split widths", "[fd-performance]")
{
    const double maturity = 91.0 / 365.0;
    const double middle = maturity / 2.0;
    const double before = std::nextafter(middle, 0.0);
    const double after = std::nextafter(middle, maturity);
    const auto grid = kiyosi::detail::make_finite_difference_time_grid(
        maturity, 4, {after, middle, before, before, 0.0, maturity});
    const std::vector expected_times{0.0, maturity / 4.0, before, middle, after, maturity * 3.0 / 4.0, maturity};
    const std::vector expected_steps{maturity / 4.0, before - maturity / 4.0, middle - before,
                                     after - middle, maturity * 3.0 / 4.0 - after, maturity / 4.0};
    CHECK(grid.times == expected_times);
    CHECK(grid.time_steps == expected_steps);
    CHECK(std::ranges::all_of(grid.time_steps, [](double dt) { return dt > 0.0; }));
    const auto stable = kiyosi::detail::check_explicit_stability(
        kiyosi::FiniteDifferenceScheme::explicit_euler, grid, 0.2, 0.04, 0.01, 3);
    REQUIRE(stable);
    const auto unstable = kiyosi::detail::check_explicit_stability(
        kiyosi::FiniteDifferenceScheme::explicit_euler, grid, 0.2, 0.04, 0.01, 200);
    REQUIRE_FALSE(unstable);
    CHECK(unstable.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("Daily finite-difference anchors replace rounded uniform duplicates", "[fd-performance]")
{
    std::vector<double> events;
    for (int day = 0; day <= 366; ++day)
        events.push_back(day / 365.0);
    const auto grid = kiyosi::detail::make_finite_difference_time_grid(366.0 / 365.0, 366, events);
    CHECK(grid.times == events);
    REQUIRE(grid.time_steps.size() == 366);
    for (const double dt : grid.time_steps)
        CHECK_THAT(dt, Catch::Matchers::WithinAbs(1.0 / 365.0, 3e-16));
}

TEST_CASE("Calendar finite-difference intervals share exact daily widths", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2024} / 1 / 1}, end{std::chrono::year{2025} / 1 / 1};
    std::vector<Date> events;
    std::vector<double> expected;
    for (int day = 0; day <= 366; ++day) {
        events.push_back(start + std::chrono::days{day});
        expected.push_back(day / 365.0);
    }
    std::ranges::reverse(events);
    events.push_back(start + std::chrono::days{1});
    for (const int steps : {1, 366}) {
        const auto grid = detail::make_finite_difference_time_grid(start_of_day(start), end, steps, events);
        CHECK(grid.times == expected);
        REQUIRE(grid.time_steps.size() == 366);
        CHECK(std::ranges::all_of(grid.time_steps, [](double dt) { return dt == 1.0 / 365.0; }));
    }
    const auto valuation = start_of_day(start) + std::chrono::hours{12} + std::chrono::microseconds{1};
    const auto intraday = detail::make_finite_difference_time_grid(valuation, end, 1, events);
    CHECK(intraday.time_steps.front() == detail::actual_365_fixed_year_fraction(valuation, start_of_day(start + std::chrono::days{1})));
    CHECK(std::ranges::all_of(intraday.time_steps | std::views::drop(1), [](double dt) { return dt == 1.0 / 365.0; }));
    const auto just_before = detail::make_finite_difference_time_grid(
        start_of_day(end) - std::chrono::microseconds{1}, end, 1, {end});
    REQUIRE(just_before.time_steps.size() == 1);
    CHECK(just_before.time_steps.front() == 1.0 / 31'536'000'000'000.0);

    const auto mixed = detail::make_finite_difference_time_grid(start_of_day(start), start + std::chrono::days{8}, 2,
                                                                {start + std::chrono::days{1}, start + std::chrono::days{2}, start + std::chrono::days{7}});
    const std::vector mixed_times{0.0, 1.0 / 365.0, 2.0 / 365.0, 4.0 / 365.0, 7.0 / 365.0, 8.0 / 365.0};
    CHECK(mixed.times == mixed_times);
    CHECK(mixed.time_steps.front() == 1.0 / 365.0);
    CHECK(mixed.time_steps.back() == 1.0 / 365.0);
    CHECK(mixed.time_steps[2] == mixed_times[3] - mixed_times[2]);
    CHECK(mixed.time_steps[3] == mixed_times[4] - mixed_times[3]);
}

TEST_CASE("Calendar finite-difference prices retain daily reference values", "[fd-performance]")
{
    using namespace kiyosi;
    const Date start{std::chrono::year{2024} / 1 / 1}, end{std::chrono::year{2025} / 1 / 1};
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 100.25, start, all_days_calendar());
    const auto check = [&](const auto& option, const auto& engine_type, const std::array<std::array<double, 3>, 3>& expected) {
        std::size_t index = 0;
        for (const auto scheme : {FiniteDifferenceScheme::crank_nicolson, FiniteDifferenceScheme::implicit_euler, FiniteDifferenceScheme::explicit_euler}) {
            using Engine = std::decay_t<decltype(engine_type)>;
            const Engine engine{{.asset_step_count = 80, .time_step_count = 1, .scheme = scheme, .asset_upper_boundary = 400.0}};
            const auto result = engine.price_with_greeks(option, context, {Greek::delta, Greek::gamma});
            REQUIRE(result);
            REQUIRE(result->has(Greek::delta));
            REQUIRE(result->has(Greek::gamma));
            CHECK(engine.price(option, context) == result->price());
            CHECK(std::abs(result->price() - expected[index][0]) < 1e-9);
            CHECK(std::abs(**result->get(Greek::delta) - expected[index][1]) < 1e-9);
            CHECK(std::abs(**result->get(Greek::gamma) - expected[index][2]) < 1e-9);
            ++index;
        }
    };
    const auto accumulator = *make_accumulator({.strike = 100.0, .knock_out_level = 115.0, .daily_quantity = 1.0, .acceleration_factor = 2.0, .effective_date = start, .expiry_date = end});
    check(accumulator, FiniteDifferenceAccumulatorEngine{}, {{{-2125.177940477314, 314.25537377676187, -24.53113696074924}, {-2120.221666260853, 314.4815991820858, -24.573439894742528}, {-2130.1933395583947, 314.0331999420339, -24.487720823425548}}});
    const auto snowball = *make_snowball_option({.knock_out_coupon_rates = {0.08, 0.08, 0.08, 0.08}, .maturity_coupon_rate = 0.06, .initial_spot = 100.0, .knock_in_level = 75.0, .knock_out_levels = {110.0, 110.0, 110.0, 110.0}, .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {Date{std::chrono::year{2024} / 4 / 1}, Date{std::chrono::year{2024} / 7 / 1}, Date{std::chrono::year{2024} / 10 / 1}, end}, .knock_in_observation_mode = KnockInObservationMode::every_trading_day, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end});
    check(snowball, FiniteDifferenceSnowballEngine{}, {{{0.9956959342110923, 0.002334647538852612, -0.00027779493631300466}, {0.9957679329994007, 0.002325382664906021, -0.00027718976198143775}, {0.9956225895060031, 0.0023440665260287138, -0.0002784043202963022}}});
    std::vector<Date> observations;
    for (int day = 1; day <= 366; ++day)
        observations.push_back(start + std::chrono::days{day});
    const auto barrier = *make_barrier_option({.option_type = OptionType::call, .strike = 100.0, .effective_date = start, .expiry_date = end, .barrier_level = 110.0, .barrier_type = BarrierType::up_and_out, .rebate = 2.0, .observation_mode = ObservationMode::scheduled, .observation_dates = observations});
    check(barrier, FiniteDifferenceBarrierEngine{}, {{{1.349334949897279, 0.05874014816127894, 0.00011939130239541559}, {1.3468904122711003, 0.058637636418814774, 0.00012654044204111787}, {1.3518636988741737, 0.0588457671037437, 0.00011178373357470883}}});
}

TEST_CASE("Finite-difference merging preserves adjacent events and later uniform widths", "[fd-performance]")
{
    const double middle = 0.5;
    const double before = std::nextafter(middle, 0.0), after = std::nextafter(middle, 1.0);
    const auto adjacent = kiyosi::detail::make_finite_difference_time_grid(1.0, 4, {after, before, before});
    CHECK(adjacent.times == std::vector<double>{0.0, 0.25, before, after, 0.75, 1.0});
    CHECK(adjacent.time_steps == std::vector<double>{0.25, before - 0.25, after - before, 0.75 - after, 0.25});

    const double event = std::nextafter(1.0 / 3.0, 1.0);
    const auto later = kiyosi::detail::make_finite_difference_time_grid(1.0, 3, {event});
    CHECK(later.times == std::vector<double>{0.0, event, 2.0 / 3.0, 1.0});
    CHECK(later.time_steps == std::vector<double>{event, 2.0 / 3.0 - event, 1.0 / 3.0});
}

TEST_CASE("Implicit Euler skips overflowing zero-weight terms", "[fd-performance]")
{
    using namespace kiyosi::detail;
    constexpr double value = 1e308;
    const DiffusionParameters parameters{0.0, 0.0, 2.0, 1.0};
    std::vector<double> old(5, value), next(5);
    FiniteDifferenceStep step{old.size()};
    for (int iteration = 0; iteration < 2; ++iteration) {
        REQUIRE(step.advance(old, next, 0.01, parameters, value, value));
        for (const double actual : next)
            CHECK_THAT(actual, Catch::Matchers::WithinRel(value, 1e-14));
        old.swap(next);
    }
}

TEST_CASE("Paired finite-difference advances match independent layers")
{
    const auto size = GENERATE(std::size_t{4}, std::size_t{7});
    const auto volatility = GENERATE(0.2, 0.001);
    const auto sinh_spacing = GENERATE(0.0, 0.2);
    constexpr double upper = 240.0;
    const double spacing = upper / static_cast<double>(size - 1);
    const std::array time_step_count{0.17, 0.17, 0.03, 0.17, 0.03, 0.11, 0.11, 0.03, 0.17};

    for (const double theta : {0.0, 1.0, 0.5}) {
        CAPTURE(theta);
        const kiyosi::detail::DiffusionParameters parameters{
            .rate = 0.03, .dividend = 0.01, .volatility = volatility, .theta = theta, .sinh_spacing = sinh_spacing};
        kiyosi::detail::LinearBoundaryStepper paired{size, upper, spacing, parameters};
        kiyosi::detail::LinearBoundaryStepper first_independent{size, upper, spacing, parameters};
        kiyosi::detail::LinearBoundaryStepper second_independent{size, upper, spacing, parameters};
        std::vector<double> first{0.2, 0.8, 1.7, 3.1, 5.2, 8.0, 11.5};
        std::vector<double> second{-3.0, -2.2, -0.7, 1.6, 4.8, 9.0, 14.3};
        first.resize(size);
        second.resize(size);
        auto expected_first = first;
        auto expected_second = second;
        std::vector<double> next_first(size), next_second(size);
        std::vector<double> next_expected_first(size), next_expected_second(size);

        for (const double dt : time_step_count) {
            REQUIRE(paired.advance_pair(first, next_first, second, next_second, dt));
            REQUIRE(first_independent.advance(expected_first, next_expected_first, dt));
            REQUIRE(second_independent.advance(expected_second, next_expected_second, dt));
            CHECK(next_first == next_expected_first);
            CHECK(next_second == next_expected_second);
            first.swap(next_first);
            second.swap(next_second);
            expected_first.swap(next_expected_first);
            expected_second.swap(next_expected_second);
        }
    }
}

TEST_CASE("Implicit finite-difference steps preserve finiteness failure guarantees")
{
    constexpr std::size_t size = 5;
    constexpr double sentinel = -123.0;
    const double infinity = std::numeric_limits<double>::infinity();

    SECTION("invalid single-layer interiors are rejected before copying")
    {
        kiyosi::detail::FiniteDifferenceStep step{size};
        const std::vector<double> old{1.0, 2.0, infinity, 4.0, 5.0};
        std::vector<double> next(size, sentinel);

        CHECK_FALSE(step.advance(old, next, 0.1, {0.03, 0.01, 0.2, 1.0}, 0.0, 10.0));
        CHECK(next[1] == sentinel);
        CHECK(next[2] == sentinel);
        CHECK(next[3] == sentinel);
    }

    SECTION("an invalid paired interior prevents either layer from being copied")
    {
        kiyosi::detail::FiniteDifferenceStep step{size};
        const std::vector<double> first_old{1.0, 2.0, 3.0, 4.0, 5.0};
        const std::vector<double> second_old{1.0, 2.0, infinity, 4.0, 5.0};
        std::vector<double> first_next(size, sentinel);
        std::vector<double> second_next(size, sentinel);
        const kiyosi::detail::DiffusionParameters parameters{
            .rate = 0.03, .dividend = 0.01, .volatility = 0.2, .theta = 1.0};

        CHECK_FALSE(step.advance_pair(first_old, first_next, second_old, second_next, 0.1,
                                      parameters, {0.0, 10.0}, {0.0, 10.0}));
        for (std::size_t index = 1; index + 1 < size; ++index) {
            CHECK(first_next[index] == sentinel);
            CHECK(second_next[index] == sentinel);
        }
    }

    SECTION("invalid boundaries are rejected after finite interiors are copied")
    {
        const auto check_boundaries = [&](double lower, double upper) {
            kiyosi::detail::FiniteDifferenceStep step{size};
            const std::vector<double> old(size, 1.0);
            std::vector<double> next(size, sentinel);

            CHECK_FALSE(step.advance(
                old, next, 0.1, {0.03, 0.01, 0.2, 1.0}, lower, upper,
                [](int) -> std::optional<double> { return 7.0; }));
            CHECK(next[1] == 7.0);
            CHECK(next[2] == 7.0);
            CHECK(next[3] == 7.0);
        };

        check_boundaries(infinity, 10.0);
        check_boundaries(0.0, infinity);
    }
}

TEST_CASE("Stretched finite-difference coefficients follow parameter changes", "[audit-fixes]")
{
    kiyosi::detail::FiniteDifferenceStep reused{5};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0};
    for (const double volatility : {0.2, 0.4, 0.2}) {
        const kiyosi::detail::DiffusionParameters parameters{
            .rate = 0.03, .dividend = 0.01, .volatility = volatility, .theta = 0.5, .sinh_spacing = 0.1};
        kiyosi::detail::FiniteDifferenceStep fresh{5};
        std::vector<double> actual(5), expected(5);
        const auto unconstrained = [](int) -> std::optional<double> { return std::nullopt; };
        REQUIRE(reused.advance(old, actual, 0.1, parameters, 0.0, 16.0, unconstrained));
        REQUIRE(fresh.advance(old, expected, 0.1, parameters, 0.0, 16.0, unconstrained));
        CHECK(actual == expected);
    }
}

TEST_CASE("Uniform finite-difference coefficients preserve quadratic evolution", "[fd-factor-cache]")
{
    kiyosi::detail::FiniteDifferenceStep reused{5};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0};
    for (const double volatility : {0.2, 0.2, 0.4, 0.2}) {
        CAPTURE(volatility);
        const kiyosi::detail::DiffusionParameters parameters{0.03, 0.01, volatility, 0.0};
        // The Black-Scholes operator maps S^2 to (rate - 2 * dividend + volatility^2) * S^2.
        const double growth = 1.0 + 0.01 * (0.03 - 2.0 * 0.01 + volatility * volatility);
        std::vector<double> actual(5);
        REQUIRE(reused.advance(old, actual, 0.01, parameters, 0.0, 16.0 * growth));
        for (std::size_t index = 0; index < old.size(); ++index)
            CHECK(std::abs(actual[index] - old[index] * growth) < 1e-12);
    }
}

TEST_CASE("Finite-difference factors follow time steps parameters and constraint masks", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    FiniteDifferenceStep reused{7};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0, 25.0, 36.0};
    for (const double spacing : {0.0, 0.1, 0.0})
        for (const double theta : {1.0, 0.5, 0.0, 0.5})
            for (const double rate : {0.03, -0.01})
                for (const double dividend : {0.01, 0.02})
                    for (const double volatility : {0.2, 0.4})
                        for (const double dt : {0.1, 0.1, 0.05, 0.1, 0.05, 0.025, 0.1, 0.025, 0.05})
                            for (const int pin : {0, 2, 2, 3, 0, 5, 5, 0})
                                for (const double value : {7.0, 8.0}) {
                                    CAPTURE(spacing, theta, rate, dividend, volatility, dt, pin, value);
                                    const DiffusionParameters parameters{rate, dividend, volatility, theta, spacing};
                                    const auto constraint = [=](int index) -> std::optional<double> {
                                        return index == pin ? std::optional{value} : std::nullopt;
                                    };
                                    FiniteDifferenceStep fresh{7};
                                    std::vector<double> actual(7), expected(7);
                                    REQUIRE(reused.advance(old, actual, dt, parameters, 0.0, 36.0, constraint));
                                    REQUIRE(fresh.advance(old, expected, dt, parameters, 0.0, 36.0, constraint));
                                    CHECK(actual == expected);
                                    if (pin != 0) CHECK(actual[static_cast<std::size_t>(pin)] == value);
                                }
}

TEST_CASE("Finite-difference factors follow constrained and unconstrained solve transitions", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    const std::vector<double> first{0.0, 1.0, 4.0, 9.0, 16.0, 25.0, 36.0};
    const std::vector<double> second{7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0};
    for (const double spacing : {0.0, 0.1})
        for (const double theta : {0.0, 0.5, 1.0}) {
            FiniteDifferenceStep reused{first.size()};
            const DiffusionParameters parameters{0.04, 0.01, 0.2, theta, spacing};
            for (const double dt : {0.1, 0.05, 0.025, 0.1, 0.05, 0.025})
                for (const bool constrained : {true, false, false, true}) {
                    CAPTURE(spacing, theta, dt, constrained);
                    const auto constraint = [=](int index) -> std::optional<double> {
                        return constrained && index >= 4 ? std::optional{8.0} : std::nullopt;
                    };
                    std::vector<double> actual(first.size()), expected(first.size());
                    FiniteDifferenceStep reference{first.size()};
                    REQUIRE(reference.advance(first, expected, dt, parameters, 0.0, 36.0, constraint));
                    if (constrained)
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0, constraint));
                    else
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0));
                    CHECK(actual == expected);

                    // Pairing must discard a previously pinned region in either layer.
                    std::vector<double> actual_second(second.size()), expected_second(second.size());
                    FiniteDifferenceStep first_reference{first.size()}, second_reference{second.size()};
                    const auto free_node = [](int) -> std::optional<double> { return std::nullopt; };
                    REQUIRE(first_reference.advance(first, expected, dt, parameters, 0.0, 36.0, free_node));
                    REQUIRE(second_reference.advance(second, expected_second, dt, parameters, 7.0, 1.0, free_node));
                    REQUIRE(reused.advance_pair(first, actual, second, actual_second, dt, parameters, {0.0, 36.0}, {7.0, 1.0}));
                    CHECK(actual == expected);
                    CHECK(actual_second == expected_second);
                    if (constrained) {
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0, constraint));
                        REQUIRE(reused.advance(first, actual, dt, parameters, 0.0, 36.0));
                        CHECK(actual == expected);
                    }
                }
        }
}

TEST_CASE("Finite-difference factor cache recovers after failed solves", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    FiniteDifferenceStep reused{5};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0};
    std::vector<double> actual(5), expected(5);
    const DiffusionParameters valid{0.03, 0.01, 0.2, 0.5};
    REQUIRE(reused.advance(old, actual, 0.1, valid, 0.0, 16.0));
    REQUIRE(reused.advance(old, actual, 0.05, valid, 0.0, 16.0));
    REQUIRE(reused.advance(old, actual, 0.025, valid, 0.0, 16.0));
    for (int attempt = 0; attempt < 3; ++attempt) {
        CHECK_FALSE(reused.advance(old, actual, 0.1, {-10.0, -10.0, 0.0, 1.0}, 0.0, 16.0));
        auto invalid_old = old;
        invalid_old[2] = std::numeric_limits<double>::infinity();
        CHECK_FALSE(reused.advance(invalid_old, actual, 0.1, valid, 0.0, 16.0));
        for (const double dt : {0.1, 0.05, 0.025, 0.05, 0.1, 0.025}) {
            FiniteDifferenceStep fresh{5};
            REQUIRE(reused.advance(old, actual, dt, valid, 0.0, 16.0));
            REQUIRE(fresh.advance(old, expected, dt, valid, 0.0, 16.0));
            CHECK(actual == expected);
        }
    }
}

TEST_CASE("Finite-difference fixed edges preserve the full system equations", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0, 25.0, 36.0};
    for (const double theta : {0.0, 0.5, 1.0}) {
        FiniteDifferenceStep step{old.size()};
        for (const int mask : {0, 3, 3, 24, 27, 31, 30, 15, 21, 0})
            for (const double pin : {7.0, 8.0}) {
                CAPTURE(theta, mask, pin);
                std::vector<double> next(old.size());
                REQUIRE(step.advance(old, next, 0.1, {0.04, 0.01, 0.2, theta}, 0.0, 50.0,
                                     [=](int index) -> std::optional<double> {
                                         return mask & (1 << (index - 1)) ? std::optional{pin} : std::nullopt;
                                     }));
                CHECK(next.front() == 0.0);
                CHECK(next.back() == 50.0);
                for (std::size_t index = 1; index + 1 < old.size(); ++index) {
                    if (mask & (1 << (index - 1))) {
                        CHECK(next[index] == pin);
                        continue;
                    }
                    const double node = static_cast<double>(index);
                    const double a = 0.02 * node * node - 0.015 * node;
                    const double b = -0.04 * node * node - 0.04;
                    const double c = 0.02 * node * node + 0.015 * node;
                    const double lhs = next[index] - theta * 0.1 *
                                                         (a * next[index - 1] + b * next[index] + c * next[index + 1]);
                    const double rhs = old[index] + (1.0 - theta) * 0.1 *
                                                        (a * old[index - 1] + b * old[index] + c * old[index + 1]);
                    CHECK(std::abs(lhs - rhs) < 1e-12);
                }
            }
        std::vector<double> next(old.size(), -123.0);
        CHECK_FALSE(step.advance(old, next, 0.1, {0.04, 0.01, 0.2, theta}, 0.0, 50.0,
                                 [](int) -> std::optional<double> { return std::numeric_limits<double>::infinity(); }));
        for (std::size_t index = 1; index + 1 < next.size(); ++index)
            CHECK(next[index] == (theta == 0.0 ? std::numeric_limits<double>::infinity() : -123.0));
        REQUIRE(step.advance(old, next, 0.1, {0.04, 0.01, 0.2, theta}, 0.0, 50.0));
        CHECK(std::ranges::all_of(next, [](double value) { return std::isfinite(value); }));
    }
}
