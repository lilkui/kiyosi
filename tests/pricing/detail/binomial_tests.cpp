#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>

#include <bit>
#include <array>
#include <cstdint>
#include <limits>
#include <vector>

#include "pricing/detail/binomial.hpp"

namespace {
using namespace kiyosi::detail;

TEST_CASE("Binomial rollback discounts continuation and applies American exercise", "[binomial-kernel]")
{
    const bool american = GENERATE(false, true);
    const auto example = GENERATE(std::array{0.0, 0.5, 2.0, 8.0},
                                  std::array{0.25, 0.875, 3.5, 14.0},
                                  std::array{1.0, 2.0, 8.0, 32.0});
    const double sign = GENERATE(-1.0, 1.0);
    std::vector<double> values{1.0, 4.0, 16.0, 64.0};
    const auto rollback = american ? &binomial_rollback<true> : &binomial_rollback<false>;
    REQUIRE(rollback(values, 0.5, example[0], 100.0, 1.25, 110.0, sign));
    CHECK(values[0] == (american && sign < 0.0 ? 10.0 : example[1]));
    CHECK(values[1] == (american && sign > 0.0 ? 15.0 : example[2]));
    CHECK(values[2] == (american && sign > 0.0 ? 46.25 : example[3]));
    CHECK(values[3] == 64.0);
}

TEST_CASE("Binomial rollback detects non-finite values in every position", "[binomial-kernel]")
{
    const bool american = GENERATE(false, true);
    const double invalid = GENERATE(std::numeric_limits<double>::infinity(),
                                    std::numeric_limits<double>::quiet_NaN(),
                                    std::numeric_limits<double>::max());
    const auto rollback = american ? &binomial_rollback<true> : &binomial_rollback<false>;
    for (std::size_t index = 0; index < 8; ++index) {
        CAPTURE(index);
        std::vector<double> values(8, 1.0);
        values[index] = invalid;
        CHECK_FALSE(rollback(values, 4.0, 0.5, 100.0, 1.01, 100.0, 1.0));
    }
}

TEST_CASE("Binomial rollback preserves subnormals and signed zeros", "[binomial-kernel]")
{
    const bool american = GENERATE(false, true);
    const double tiny = std::numeric_limits<double>::denorm_min();
    std::vector<double> actual{tiny, 3.0 * tiny, 5.0 * tiny, 7.0 * tiny, -0.0, -0.0};
    const std::array expected{2.0 * tiny, 4.0 * tiny, 6.0 * tiny, 4.0 * tiny, -0.0, -0.0};
    const auto rollback = american ? &binomial_rollback<true> : &binomial_rollback<false>;
    REQUIRE(rollback(actual, 1.0, 0.5, 100.0, 1.0, 100.0, 1.0));
    for (std::size_t index = 0; index < actual.size(); ++index)
        CHECK(std::bit_cast<std::uint64_t>(actual[index]) == std::bit_cast<std::uint64_t>(expected[index]));
}

} // namespace
