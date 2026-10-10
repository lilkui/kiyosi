#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <optional>
#include <vector>

#include "pricing/detail/fd_scheme.hpp"
#include "pricing/detail/fd_grid.hpp"

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

TEST_CASE("Finite-difference spline interpolation stays within neighboring payoff values", "[audit-fixes]")
{
    const kiyosi::detail::SpatialGrid grid{8.0, 1.0, 8};
    for (const double sign : {1.0, -1.0}) {
        std::vector<double> values(grid.size());
        for (std::size_t index = 0; index < values.size(); ++index)
            values[index] = std::max(sign * (static_cast<double>(index) - 4.0), 0.0);
        for (int sample = 0; sample <= 32; ++sample) {
            const double spot = static_cast<double>(sample) / 4.0;
            const auto index = std::min(static_cast<std::size_t>(spot), values.size() - 2);
            const double value = grid.interpolate(values, spot);
            CHECK(value >= std::min(values[index], values[index + 1]));
            CHECK(value <= std::max(values[index], values[index + 1]));
        }
        CHECK(grid.interpolate(values, sign > 0.0 ? 3.75 : 4.25) == 0.0);
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
    for (const double maturity : {91.0 / 365.0, 1.0, 2.75}) {
        for (const int steps : {1, 3, 80, 200, 1000}) {
            CAPTURE(maturity, steps);
            const auto grid = kiyosi::detail::make_finite_difference_time_grid(maturity, steps);
            REQUIRE(grid.time_steps.size() == static_cast<std::size_t>(steps));
            CHECK(std::ranges::all_of(grid.time_steps, [=](double dt) { return dt == maturity / steps; }));
            CHECK(grid.times.front() == 0.0);
            CHECK(grid.times.back() == maturity);
        }
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

TEST_CASE("Paired finite-difference advances match independent layers")
{
    const auto size = GENERATE(std::size_t{4}, std::size_t{7});
    const auto volatility = GENERATE(0.2, 0.001);
    const auto sinh_spacing = GENERATE(0.0, 0.2);
    constexpr double upper = 240.0;
    const double spacing = upper / static_cast<double>(size - 1);
    const std::array time_step_count{0.17, 0.17, 0.03, 0.11, 0.11};

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
                        for (const double dt : {0.1, 0.1, 0.05})
                            for (const int pin : {0, 2, 2, 3, 0})
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

TEST_CASE("Finite-difference factor cache recovers after failed solves", "[fd-factor-cache]")
{
    using namespace kiyosi::detail;
    FiniteDifferenceStep reused{5};
    const std::vector<double> old{0.0, 1.0, 4.0, 9.0, 16.0};
    std::vector<double> actual(5), expected(5);
    const DiffusionParameters valid{0.03, 0.01, 0.2, 0.5};
    REQUIRE(reused.advance(old, actual, 0.1, valid, 0.0, 16.0));
    for (int attempt = 0; attempt < 2; ++attempt) {
        CHECK_FALSE(reused.advance(old, actual, 0.1, {-10.0, -10.0, 0.0, 1.0}, 0.0, 16.0));
        auto invalid_old = old;
        invalid_old[2] = std::numeric_limits<double>::infinity();
        CHECK_FALSE(reused.advance(invalid_old, actual, 0.1, valid, 0.0, 16.0));
        FiniteDifferenceStep fresh{5};
        REQUIRE(reused.advance(old, actual, 0.1, valid, 0.0, 16.0));
        REQUIRE(fresh.advance(old, expected, 0.1, valid, 0.0, 16.0));
        CHECK(actual == expected);
    }
}
