#include <catch2/catch_test_macros.hpp>

#include <array>
#include <limits>
#include <optional>
#include <vector>

#include "pricing/detail/fd_scheme.hpp"
#include "pricing/detail/fd_grid.hpp"

TEST_CASE("Finite-difference grids preserve exact expiry without replaying terminal events", "[cross-validation]")
{
    const double maturity = 91.0 / 365.0;
    for (const int steps : {400, 800, 1600}) {
        CAPTURE(steps);
        const auto grid = kiyosi::detail::finite_difference_grid(maturity, steps, {0.0, maturity});
        CHECK(grid.front() == 0.0);
        CHECK(grid.back() == maturity);
        CHECK(grid.size() == static_cast<std::size_t>(steps + 1));
    }
}

TEST_CASE("Paired finite-difference advances match independent layers")
{
    constexpr std::size_t size = 7;
    constexpr double upper = 240.0;
    constexpr double spacing = upper / static_cast<double>(size - 1);
    const std::array time_steps{0.17, 0.03, 0.11};

    for (const double theta : {0.0, 1.0, 0.5}) {
        CAPTURE(theta);
        const kiyosi::detail::DiffusionParameters parameters{
            .rate = 0.03, .dividend = 0.01, .volatility = 0.2, .theta = theta};
        kiyosi::detail::LinearBoundaryStepper paired{size, upper, spacing, parameters};
        kiyosi::detail::LinearBoundaryStepper first_independent{size, upper, spacing, parameters};
        kiyosi::detail::LinearBoundaryStepper second_independent{size, upper, spacing, parameters};
        std::vector<double> first{0.2, 0.8, 1.7, 3.1, 5.2, 8.0, 11.5};
        std::vector<double> second{-3.0, -2.2, -0.7, 1.6, 4.8, 9.0, 14.3};
        auto expected_first = first;
        auto expected_second = second;
        std::vector<double> next_first(size), next_second(size);
        std::vector<double> next_expected_first(size), next_expected_second(size);

        for (const double dt : time_steps) {
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

        CHECK_FALSE(step.advance(old, next, 0.1, 0.03, 0.01, 0.2, 1.0, 0.0, 10.0));
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
                old, next, 0.1, 0.03, 0.01, 0.2, 1.0, lower, upper,
                [](int) -> std::optional<double> { return 7.0; }));
            CHECK(next[1] == 7.0);
            CHECK(next[2] == 7.0);
            CHECK(next[3] == 7.0);
        };

        check_boundaries(infinity, 10.0);
        check_boundaries(0.0, infinity);
    }
}
