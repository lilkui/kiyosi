#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <tuple>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include <kiyosi/pricing/detail/accumulator_program.hpp>

#include "support/common.hpp"
#include "pricing/engines/monte_carlo_discount.hpp"

namespace {
using namespace kiyosi;
using test::day;

TEST_CASE("Accumulator fixed paths settle all accrued units once at knock-out or expiry", "[structured-cashflows]")
{
    struct Scenario {
        double barrier;
        std::vector<double> spots;
        std::vector<double> quantities;
        bool knocked_out;
        double settlement;
    };
    for (const auto& scenario : {
             Scenario{110, {110, 20}, {3}, true, 30},
             Scenario{110, {90, 100, 110, 20}, {5, 6, 6}, true, 60},
             Scenario{110, {90, 100, 109}, {5, 6, 7}, false, 63},
             Scenario{90, {90, 20}, {3}, true, -30},
             Scenario{90, {80, 90, 20}, {5, 5}, true, -50}}) {
        const auto start = day(2025, 1, 1);
        const auto option = make_accumulator({100, scenario.barrier, 1, 2, 3, start, day(2025, 1, 4)}).value();
        double quantity = option.accumulated_quantity();
        double spot = 0;
        bool knocked_out = false;
        std::size_t processed = 0;
        for (const double observed : scenario.spots) {
            spot = observed;
            knocked_out = detail::observe_accumulator_path(spot, option.strike(), option.knock_out_level(), option.daily_quantity(), option.acceleration_factor(), quantity);
            REQUIRE(processed < scenario.quantities.size());
            CHECK(quantity == scenario.quantities[processed++]);
            if (knocked_out) break;
        }
        CHECK(processed == scenario.quantities.size());
        CHECK(knocked_out == scenario.knocked_out);
        const double discount = std::exp(-0.04 * static_cast<double>(processed) / 365.0);
        const detail::MonteCarloDiscount payment{discount, std::log(discount)};
        CHECK(payment.apply(quantity, spot - option.strike()) == Catch::Approx(discount * scenario.settlement).epsilon(0).margin(1e-12));
    }
}

TEST_CASE("Accumulator path events distinguish adjacent strike and knock-out values", "[structured-cashflows]")
{
    for (const auto& [spot, expected_quantity, expected_stop] : std::array{
             std::tuple{std::nextafter(100.0, 0.0), 5.0, false}, std::tuple{100.0, 4.0, false},
             std::tuple{std::nextafter(100.0, 110.0), 4.0, false}, std::tuple{std::nextafter(110.0, 0.0), 4.0, false},
             std::tuple{110.0, 3.0, true}, std::tuple{std::nextafter(110.0, 120.0), 3.0, true}}) {
        CAPTURE(spot);
        double quantity = 3;
        CHECK(detail::observe_accumulator_path(spot, 100, 110, 1, 2, quantity) == expected_stop);
        CHECK(quantity == expected_quantity);
    }
}

TEST_CASE("Accumulator public engines exclude already processed intraday purchases", "[structured-cashflows]")
{
    const auto start = day(2025, 1, 1), end = day(2025, 1, 2);
    const auto option = make_accumulator({100, 110, 1, 2, 3, start, end}).value();
    const auto monte_carlo = make_monte_carlo_accumulator_engine({32, 7}).value();
    for (const auto& [valuation, expected] : std::array{
             std::pair{start_of_day(start), -70.0}, std::pair{start_of_day(start) + std::chrono::hours{12}, -50.0},
             std::pair{start_of_day(end), -50.0}}) {
        const auto context = make_pricing_context(*make_bsm_parameters(0, 0, 1e-8), 90, valuation, all_days_calendar()).value();
        for (const auto& price : {monte_carlo.price(option, context), FiniteDifferenceAccumulatorEngine{}.price(option, context), QuadratureAccumulatorEngine{}.price(option, context)}) {
            REQUIRE(price);
            CHECK(*price == Catch::Approx(expected).epsilon(0).margin(1e-5));
        }
    }
    for (const double barrier : {90.0, 110.0}) {
        const auto current = make_accumulator({100, barrier, 1, 2, 3, start, end}).value();
        const auto context = make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), barrier, start, all_days_calendar()).value();
        for (const auto& price : {monte_carlo.price(current, context), FiniteDifferenceAccumulatorEngine{}.price(current, context), QuadratureAccumulatorEngine{}.price(current, context)}) {
            REQUIRE(price);
            CHECK(*price == 3 * (barrier - 100));
        }
    }
}
} // namespace
