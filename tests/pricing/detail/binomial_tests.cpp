#include <catch2/catch_test_macros.hpp>
#include <catch2/generators/catch_generators.hpp>

#include <bit>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <vector>

#include "pricing/detail/binomial.hpp"

namespace {
using namespace kiyosi::detail;

TEST_CASE("Binomial rollback preserves scalar bits and every remainder", "[binomial-kernel]")
{
    const bool american = GENERATE(false, true);
    const int steps = GENERATE(1, 2, 3, 4, 5, 6, 7, 8, 17, 128, 257, 1001);
    const double probability = GENERATE(0.0, 0.37, 1.0);
    const double sign = GENERATE(-1.0, 1.0);
    const double up = std::exp(0.2 / std::sqrt(static_cast<double>(steps)));
    const double down = 1.0 / up;
    const double discount = std::exp(-0.04 / steps);
    std::vector<double> expected(static_cast<std::size_t>(steps) + 1);
    double spot = 100.0 * std::pow(down, steps);
    for (auto& value : expected) {
        value = std::max(sign * (spot - 100.0), 0.0);
        spot *= up * up;
    }
    auto actual = expected;
    const auto scalar = american ? &binomial_rollback_scalar<true> : &binomial_rollback_scalar<false>;
    const auto rollback = binomial_rollback(american);
    spot = 100.0 * std::pow(down, steps - 1);
    for (int level = steps - 1; level >= 0; --level) {
        if (level < steps - 1) spot *= up;
        const auto count = static_cast<std::size_t>(level) + 2;
        REQUIRE(scalar(std::span{expected}.first(count), discount, probability, spot, up * up, 100.0, sign));
        REQUIRE(rollback(std::span{actual}.first(count), discount, probability, spot, up * up, 100.0, sign));
        CAPTURE(level);
        CHECK(std::ranges::equal(actual, expected, [](double left, double right) {
            return std::bit_cast<std::uint64_t>(left) == std::bit_cast<std::uint64_t>(right);
        }));
    }
}

TEST_CASE("Binomial rollback detects non-finite values in every lane and tail", "[binomial-kernel]")
{
    const bool american = GENERATE(false, true);
    const double invalid = GENERATE(std::numeric_limits<double>::infinity(),
                                    std::numeric_limits<double>::quiet_NaN(),
                                    std::numeric_limits<double>::max());
    for (std::size_t lane = 0; lane < 8; ++lane) {
        CAPTURE(lane);
        std::vector<double> values(8, 1.0);
        values[lane] = invalid;
        CHECK_FALSE(binomial_rollback(american)(values, 4.0, 0.5, 100.0, 1.01, 100.0, 1.0));
    }
}

TEST_CASE("Binomial rollback preserves subnormals and signed zeros", "[binomial-kernel]")
{
    const bool american = GENERATE(false, true);
    const double probability = GENERATE(0.0, 0.5, 1.0);
    const double sign = GENERATE(-1.0, 1.0);
    const double tiny = std::numeric_limits<double>::denorm_min();
    std::vector<double> expected{-0.0, -0.0, tiny, -tiny, 2.0 * tiny, 0.0, -0.0, -0.0};
    auto actual = expected;
    const auto scalar = american ? &binomial_rollback_scalar<true> : &binomial_rollback_scalar<false>;
    REQUIRE(scalar(expected, 1.0, probability, 100.0, 1.0, 100.0, sign));
    REQUIRE(binomial_rollback(american)(actual, 1.0, probability, 100.0, 1.0, 100.0, sign));
    for (std::size_t index = 0; index < actual.size(); ++index)
        CHECK(std::bit_cast<std::uint64_t>(actual[index]) == std::bit_cast<std::uint64_t>(expected[index]));
}

} // namespace
