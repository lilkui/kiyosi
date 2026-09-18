#include <catch2/catch_test_macros.hpp>

#include <array>
#include <vector>

#include "pricing/detail/fd_scheme.hpp"

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
