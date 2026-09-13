#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>
#include <array>
#include <chrono>
#include <limits>
#include <string>
#include <type_traits>
#include <vector>
#include <kiyosi/kiyosi.hpp>
#include "support/common.hpp"

namespace {

using kiyosi::test::day;

TEST_CASE("Binary barriers expose observation intervals and reject invalid at-hit terms")
{
    const auto valuation = day(2025, 1, 6);
    const auto expiry = valuation + std::chrono::days{365};
    const auto observations = std::vector<kiyosi::date>{valuation + std::chrono::days{30}, valuation + std::chrono::days{180}};
    const auto binary = kiyosi::make_binary_barrier_option(
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_in, 10.0,
        false, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled, observations);
    REQUIRE(binary.has_value());
    CHECK_THAT(binary->observation_interval(), Catch::Matchers::WithinAbs(180.0 / 365.0 / 2.0, 1e-12));
    CHECK(kiyosi::make_binary_barrier_option(
              kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out, 10.0,
              false, kiyosi::rebate_timing::at_hit)
              .error()
              .category == kiyosi::error_category::invalid_option);
    CHECK(kiyosi::make_binary_barrier_option(
              std::nullopt, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_in, 89.0, true,
              kiyosi::rebate_timing::at_hit)
              .error()
              .category == kiyosi::error_category::invalid_parameter);
}

} // namespace
