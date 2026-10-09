#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <tuple>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {

using kiyosi::test::day;
using kiyosi::test::greek_value;

constexpr auto valuation = day(2025, 1, 6);
const auto expiry_date = valuation + std::chrono::days{365};

kiyosi::PricingContext context(double spot = 100.0, double rate = 0.04,
                               double dividend = 0.01, double volatility = 0.3,
                               kiyosi::Date value_date = valuation)
{
    const auto parameters = kiyosi::make_bsm_parameters(rate, dividend, volatility);
    REQUIRE(parameters);
    const auto result = kiyosi::make_pricing_context(*parameters, spot, value_date);
    REQUIRE(result);
    // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
    return *result;
}

kiyosi::PricingResult analytic(kiyosi::OptionType type, double spot = 100.0, double rate = 0.04,
                               double dividend = 0.01, double volatility = 0.3,
                               kiyosi::Date value_date = valuation, kiyosi::Date option_expiry = expiry_date,
                               double strike = 100.0)
{
    const auto option = kiyosi::make_european_option(type, strike, value_date, option_expiry);
    REQUIRE(option);
    const auto result = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(
        *option, context(spot, rate, dividend, volatility, value_date), kiyosi::GreeksRequest{true});
    REQUIRE(result);
    return *result;
}

double difference(double left, double right)
{
    return std::abs(left - right);
}

void check_close(double actual, double expected, double absolute = 1e-7, double relative = 1e-5)
{
    CHECK(difference(actual, expected) <= absolute + relative * std::abs(expected));
}

double value(kiyosi::OptionType type, double spot, double rate, double dividend,
             double volatility, kiyosi::Date value_date, kiyosi::Date option_expiry, double strike = 100.0)
{
    const auto option = kiyosi::make_european_option(type, strike, value_date, option_expiry);
    REQUIRE(option);
    const auto result = kiyosi::AnalyticVanillaEngine{}.price(
        *option, context(spot, rate, dividend, volatility, value_date));
    REQUIRE(result);
    return *result;
}

double delta(kiyosi::OptionType type, double spot, double rate, double dividend,
             double volatility, kiyosi::Date value_date, kiyosi::Date option_expiry, double strike = 100.0)
{
    return greek_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                       kiyosi::Greek::delta);
}

double gamma(kiyosi::OptionType type, double spot, double rate, double dividend,
             double volatility, kiyosi::Date value_date, kiyosi::Date option_expiry, double strike = 100.0)
{
    return greek_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                       kiyosi::Greek::gamma);
}

TEST_CASE("Numerical time Greeks center clipped stencils on valuation", "[audit-fixes]")
{
    using namespace kiyosi;
    struct QuadraticTimeEngine {
        Result<double> price(const EuropeanOption& option, const PricingContext& market) const
        {
            const double days = std::chrono::duration<double, std::ratio<86400>>{
                market.valuation_time() - start_of_day(option.effective_date())}
                                    .count();
            return days * days * market.spot_price() * market.spot_price();
        }
    };
    const auto option = *make_european_option(OptionType::call, 100.0, valuation, valuation + std::chrono::days{10});
    // Price, delta and gamma are quadratic in time: a centered derivative is exact at both life boundaries.
    for (const auto elapsed : {std::chrono::microseconds{1}, std::chrono::microseconds{10},
                              std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::hours{12}),
                              std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::hours{228})}) {
        const double days = std::chrono::duration<double, std::ratio<86400>>{elapsed}.count();
        const auto time = start_of_day(valuation) + elapsed;
        const auto market = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.3), 100.0, time);
        const auto result = calculate_numerical_greeks(QuadraticTimeEngine{}, option, market);
        REQUIRE(result);
        check_close(greek_value(*result, Greek::theta), 2.0 * days * 10000.0, 1e-12);
        check_close(greek_value(*result, Greek::charm), 4.0 * days * 100.0, 1e-12);
        check_close(greek_value(*result, Greek::color), 4.0 * days, 1e-14);
    }
    const auto first = calculate_numerical_greeks(QuadraticTimeEngine{}, option, context());
    REQUIRE(first);
    check_close(greek_value(*first, Greek::theta), 10000.0);
}

TEST_CASE("Finite-difference spot Greeks remain accurate at and between grid nodes")
{
    const auto option = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                      .strike = 100.0,
                                                      .effective_date = valuation,
                                                      .expiry_date = expiry_date,
                                                      .barrier_level = 80.0,
                                                      .barrier_type = kiyosi::BarrierType::down_and_out});
    const kiyosi::FiniteDifferenceBarrierEngine finite{800, 1000};
    for (const double spot : {95.0, 95.1}) {
        CAPTURE(spot);
        const auto market = context(spot, 0.03, 0.02, 0.2);
        const auto expected = kiyosi::AnalyticBarrierEngine{}.price_with_greeks(
            option, market, {kiyosi::Greek::gamma, kiyosi::Greek::speed});
        const auto actual = finite.price_with_greeks(
            option, market, {kiyosi::Greek::gamma, kiyosi::Greek::speed});
        REQUIRE(expected);
        REQUIRE(actual);
        check_close(greek_value(*actual, kiyosi::Greek::gamma),
                    greek_value(*expected, kiyosi::Greek::gamma), 2e-5, 0.01);
        check_close(greek_value(*actual, kiyosi::Greek::speed),
                    greek_value(*expected, kiyosi::Greek::speed), 2e-5, 0.05);
    }
}

TEST_CASE("Finite-difference numerical Greeks keep the automatic asset grid fixed")
{
    const auto option = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const kiyosi::FiniteDifferenceVanillaEngine finite{800, 1000};
    for (const double spot : {95.0, 95.1, 100.0, 100.1, 110.0}) {
        CAPTURE(spot);
        const auto market = context(spot, 0.03, 0.02, 0.2);
        const auto expected = kiyosi::AnalyticVanillaEngine{}.price_with_greeks(option, market, true);
        const auto numerical = kiyosi::calculate_numerical_greeks(finite, option, market);
        const auto joint = finite.price_with_greeks(option, market, {kiyosi::Greek::speed});
        REQUIRE(expected);
        REQUIRE(numerical);
        REQUIRE(joint);
        check_close(greek_value(*numerical, kiyosi::Greek::gamma), greek_value(*expected, kiyosi::Greek::gamma), 2e-5, 0.01);
        check_close(greek_value(*numerical, kiyosi::Greek::speed), greek_value(*expected, kiyosi::Greek::speed), 2e-5, 0.05);
        CHECK(greek_value(*joint, kiyosi::Greek::speed) == greek_value(*numerical, kiyosi::Greek::speed));
        CHECK(numerical->price() == *finite.price(option, market));
        CHECK_FALSE(finite.settings().asset_upper_boundary);
    }
}

TEST_CASE("Finite-difference Greek grids preserve extreme expiry settlements")
{
    const auto market = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.03, 0.02, 0.2), 1e308, expiry_date);
    const auto vanilla = *kiyosi::make_european_option(kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 100.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = expiry_date,
                                                       .barrier_level = 80.0,
                                                       .barrier_type = kiyosi::BarrierType::down_and_out,
                                                       .touch_state = kiyosi::BarrierTouchState::untouched});
    const auto check = [&](const auto& engine, const auto& option) {
        const auto price = engine.price(option, market);
        const auto joint = engine.price_with_greeks(option, market, {kiyosi::Greek::gamma});
        REQUIRE(price);
        REQUIRE(joint);
        CHECK(joint->price() == *price);
        CHECK_FALSE(joint->has(kiyosi::Greek::gamma));
    };
    check(kiyosi::FiniteDifferenceVanillaEngine{}, vanilla);
    check(kiyosi::FiniteDifferenceBarrierEngine{}, barrier);
}

struct RejectingMixedBumpEngine {
    kiyosi::Result<double> price(
        const kiyosi::EuropeanOption&, const kiyosi::PricingContext& market) const
    {
        if (market.spot_price() > 100.0 && market.model_parameters().volatility() > 0.3)
            return std::unexpected(kiyosi::Error{
                kiyosi::ErrorCategory::invalid_schedule, "feasible mixed bump rejected"});
        return market.spot_price() + market.model_parameters().volatility();
    }
};

TEST_CASE("Analytic Greeks agree with central finite differences")
{
    const double spot_step = 0.01;
    const double volatility_step = 1e-4;
    const auto previous_day = valuation - std::chrono::days{1};
    const auto next_day = valuation + std::chrono::days{1};

    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {

        const double center = value(type, 100.0, 0.04, 0.01, 0.3, valuation, expiry_date);
        const double up = value(type, 100.0 + spot_step, 0.04, 0.01, 0.3, valuation, expiry_date);
        const double down = value(type, 100.0 - spot_step, 0.04, 0.01, 0.3, valuation, expiry_date);
        const double up_two = value(type, 100.0 + 2.0 * spot_step, 0.04, 0.01, 0.3, valuation, expiry_date);
        const double down_two = value(type, 100.0 - 2.0 * spot_step, 0.04, 0.01, 0.3, valuation, expiry_date);
        const double delta_fd = (up - down) / (2.0 * spot_step);
        const double gamma_fd = (up - 2.0 * center + down) / (spot_step * spot_step);
        const double speed_fd = (up_two - 2.0 * up + 2.0 * down - down_two) /
                                (2.0 * spot_step * spot_step * spot_step);

        const auto result = analytic(type);
        check_close(greek_value(result, kiyosi::Greek::delta), delta_fd, 2e-7, 2e-4);
        check_close(greek_value(result, kiyosi::Greek::gamma), gamma_fd, 2e-7, 2e-4);
        check_close(greek_value(result, kiyosi::Greek::speed), speed_fd, 2e-6, 2e-3);

        const double theta_fd = (value(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry_date) -
                                 value(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry_date)) /
                                2.0;
        const double charm_fd = (delta(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry_date) -
                                 delta(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry_date)) /
                                2.0;
        const double color_fd = (gamma(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry_date) -
                                 gamma(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry_date)) /
                                2.0;
        check_close(greek_value(result, kiyosi::Greek::theta), theta_fd, 2e-6, 2e-3);
        check_close(greek_value(result, kiyosi::Greek::charm), charm_fd, 2e-6, 2e-3);
        check_close(greek_value(result, kiyosi::Greek::color), color_fd, 2e-6, 2e-3);

        const double vega_fd = (value(type, 100.0, 0.04, 0.01, 0.3 + volatility_step, valuation, expiry_date) -
                                value(type, 100.0, 0.04, 0.01, 0.3 - volatility_step, valuation, expiry_date)) /
                               (2.0 * volatility_step * 100.0);
        const double vanna_fd = (delta(type, 100.0, 0.04, 0.01, 0.3 + volatility_step, valuation, expiry_date) -
                                 delta(type, 100.0, 0.04, 0.01, 0.3 - volatility_step, valuation, expiry_date)) /
                                (2.0 * volatility_step * 100.0);
        const double zomma_fd = (gamma(type, 100.0, 0.04, 0.01, 0.3 + volatility_step, valuation, expiry_date) -
                                 gamma(type, 100.0, 0.04, 0.01, 0.3 - volatility_step, valuation, expiry_date)) /
                                (2.0 * volatility_step * 100.0);
        const double rho_step = 1e-4;
        const double rho_fd = (value(type, 100.0, 0.04 + rho_step, 0.01, 0.3, valuation, expiry_date) -
                               value(type, 100.0, 0.04 - rho_step, 0.01, 0.3, valuation, expiry_date)) /
                              (2.0 * rho_step * 100.0);
        check_close(greek_value(result, kiyosi::Greek::vega), vega_fd, 2e-6, 2e-4);
        check_close(greek_value(result, kiyosi::Greek::vanna), vanna_fd, 2e-6, 2e-3);
        check_close(greek_value(result, kiyosi::Greek::zomma), zomma_fd, 2e-6, 2e-3);
        check_close(greek_value(result, kiyosi::Greek::rho), rho_fd, 2e-6, 2e-4);
    }
}

TEST_CASE("Analytic and numerical analytics share Greek conventions")
{
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation - std::chrono::days{30}, expiry_date);
    const auto market = context();
    const kiyosi::AnalyticVanillaEngine engine;
    const auto analytic_result = *engine.price_with_greeks(option, market, kiyosi::GreeksRequest{true});
    const auto numerical_result = *kiyosi::calculate_numerical_greeks(engine, option, market);

    for (std::size_t index = 0; index < kiyosi::greek_count; ++index) {
        const auto measure = static_cast<kiyosi::Greek>(index);
        INFO("Greek index: " << index);
        check_close(greek_value(numerical_result, measure), greek_value(analytic_result, measure),
                    2e-6, 2e-3);
    }
}

TEST_CASE("Numerical analytics retain valid results at stencil boundaries")
{
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const kiyosi::AnalyticVanillaEngine engine;

    SECTION("low volatility")
    {
        const auto market = context(100.0, 0.05, 0.02, 0.00005);
        const auto direct = *engine.price(option, market);
        const auto result = kiyosi::calculate_numerical_greeks(engine, option, market);

        REQUIRE(result);
        check_close(result->price(),
                    direct);
        CHECK(result->has(kiyosi::Greek::delta));
        CHECK(result->has(kiyosi::Greek::rho));
        CHECK_FALSE(result->has(kiyosi::Greek::vega));
        CHECK_FALSE(result->has(kiyosi::Greek::vanna));
        CHECK_FALSE(result->has(kiyosi::Greek::zomma));
    }

    SECTION("low spot")
    {
        const auto market = context(0.005);
        const auto direct = *engine.price(option, market);
        const auto result = kiyosi::calculate_numerical_greeks(engine, option, market);

        REQUIRE(result);
        check_close(result->price(),
                    direct);
        CHECK(result->has(kiyosi::Greek::vega));
        CHECK(result->has(kiyosi::Greek::theta));
        CHECK(result->has(kiyosi::Greek::rho));
        for (const auto measure : {
                 kiyosi::Greek::delta, kiyosi::Greek::gamma,
                 kiyosi::Greek::speed, kiyosi::Greek::charm,
                 kiyosi::Greek::color, kiyosi::Greek::vanna,
                 kiyosi::Greek::zomma})
            CHECK_FALSE(result->has(measure));
    }

    SECTION("no time direction")
    {
        const auto expiring = *kiyosi::make_european_option(
            kiyosi::OptionType::call, 100.0, valuation, valuation);
        const auto result = kiyosi::calculate_numerical_greeks(engine, expiring, context());

        REQUIRE(result);
        CHECK(std::isfinite(result->price()));
        CHECK_FALSE(result->has(kiyosi::Greek::theta));
        CHECK_FALSE(result->has(kiyosi::Greek::charm));
        CHECK_FALSE(result->has(kiyosi::Greek::color));
    }
}

TEST_CASE("Numerical analytics preserve feasible bump failures")
{
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto result = kiyosi::calculate_numerical_greeks(
        RejectingMixedBumpEngine{}, option, context());

    REQUIRE_FALSE(result);
    CHECK(result.error().category == kiyosi::ErrorCategory::invalid_schedule);
    CHECK(result.error().message == "feasible mixed bump rejected");
}

TEST_CASE("Analytic pricing satisfies no-arbitrage identities")
{
    const auto call = analytic(kiyosi::OptionType::call);
    const auto put = analytic(kiyosi::OptionType::put);
    const double time = 1.0;
    check_close((call).price() - (put).price(), 100.0 * std::exp(-0.01 * time) - 100.0 * std::exp(-0.04 * time), 1e-10, 1e-10);
    check_close(greek_value(call, kiyosi::Greek::delta) - greek_value(put, kiyosi::Greek::delta), std::exp(-0.01 * time), 1e-10, 1e-10);
    check_close(greek_value(call, kiyosi::Greek::gamma), greek_value(put, kiyosi::Greek::gamma), 1e-10, 1e-10);
    check_close(greek_value(call, kiyosi::Greek::vega), greek_value(put, kiyosi::Greek::vega), 1e-10, 1e-10);

    const kiyosi::AnalyticDigitalEngine digital;
    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
        const auto cash = *kiyosi::make_cash_or_nothing_option(type, 100.0, 100.0, valuation, expiry_date);
        const auto asset = *kiyosi::make_asset_or_nothing_option(type, 100.0, valuation, expiry_date);
        const auto vanilla = analytic(type);
        const auto cash_value = *digital.price(cash, context());
        const auto asset_value = *digital.price(asset, context());
        check_close(type == kiyosi::OptionType::call ? asset_value - cash_value : cash_value - asset_value,
                    (vanilla).price(), 2e-10, 2e-10);
    }

    const kiyosi::AnalyticBarrierEngine barriers;
    for (const auto kind : {kiyosi::BarrierType::up_and_in, kiyosi::BarrierType::up_and_out,
                            kiyosi::BarrierType::down_and_in, kiyosi::BarrierType::down_and_out}) {
        const auto barrier = kind == kiyosi::BarrierType::up_and_in || kind == kiyosi::BarrierType::up_and_out
                                 ? 130.0
                                 : 75.0;
        const auto option = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                          .strike = 100.0,
                                                          .effective_date = valuation,
                                                          .expiry_date = expiry_date,
                                                          .barrier_level = barrier,
                                                          .barrier_type = kind});
        const auto paired_kind = kind == kiyosi::BarrierType::up_and_in
                                     ? kiyosi::BarrierType::up_and_out
                                 : kind == kiyosi::BarrierType::up_and_out  ? kiyosi::BarrierType::up_and_in
                                 : kind == kiyosi::BarrierType::down_and_in ? kiyosi::BarrierType::down_and_out
                                                                            : kiyosi::BarrierType::down_and_in;
        const auto paired = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                          .strike = 100.0,
                                                          .effective_date = valuation,
                                                          .expiry_date = expiry_date,
                                                          .barrier_level = barrier,
                                                          .barrier_type = paired_kind});
        check_close(*barriers.price(option, context()) +
                        *barriers.price(paired, context()),
                    (analytic(kiyosi::OptionType::call)).price(), 2e-5, 2e-5);
    }
}

TEST_CASE("Analytic prices are monotone and bounded")
{
    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
        const double short_value = value(type, 100.0, 0.0, 0.0, 0.2, valuation,
                                         valuation + std::chrono::days{182});
        const double long_value = value(type, 100.0, 0.0, 0.0, 0.2, valuation,
                                        valuation + std::chrono::days{730});
        CHECK(long_value >= short_value);
        const double low_volatility = value(type, 100.0, 0.0, 0.0, 0.1, valuation, expiry_date);
        const double high_volatility = value(type, 100.0, 0.0, 0.0, 0.4, valuation, expiry_date);
        CHECK(high_volatility >= low_volatility);

        const double price = value(type, 110.0, 0.0, 0.0, 0.2, valuation, expiry_date);
        const double intrinsic = type == kiyosi::OptionType::call ? 10.0 : 0.0;
        const double upper_bound = type == kiyosi::OptionType::call ? 110.0 : 100.0;
        CHECK(intrinsic <= price);
        CHECK(price <= upper_bound);
    }
}

TEST_CASE("Binary barrier expiry_date uses inclusive hits and strict strikes")
{
    struct ExpiryCase {
        bool asset{};
        kiyosi::BarrierType barrier{};
        std::optional<kiyosi::OptionType> type;
        double strike{};
        double level{};
        double expected{};
    };
    const std::array<ExpiryCase, 8> cases{
        ExpiryCase{false, kiyosi::BarrierType::up_and_in, kiyosi::OptionType::call, 100, 100, 0},
        {false, kiyosi::BarrierType::down_and_in, kiyosi::OptionType::put, 101, 100, 10},
        {true, kiyosi::BarrierType::up_and_in, {}, 100, 100, 100},
        {true, kiyosi::BarrierType::down_and_in, kiyosi::OptionType::put, 101, 100, 100},
        {false, kiyosi::BarrierType::up_and_out, {}, 100, 100, 0},
        {true, kiyosi::BarrierType::down_and_out, {}, 100, 100, 0},
        {false, kiyosi::BarrierType::up_and_out, {}, 100, 110, 10},
        {true, kiyosi::BarrierType::down_and_out, kiyosi::OptionType::call, 99, 90, 100},
    };
    for (const auto& item : cases) {
        const auto check = [&](const auto& option) {
            CHECK(*kiyosi::AnalyticBinaryBarrierEngine{}.price(
                      option, context(100.0, 0.04, 0.01, 0.3, expiry_date)) == item.expected);
        };
        if (item.type) {
            const kiyosi::BinaryBarrierTerms terms{.option_type = *item.type,
                                                   .strike = item.strike,
                                                   .effective_date = expiry_date,
                                                   .expiry_date = expiry_date,
                                                   .barrier_level = item.level,
                                                   .barrier_type = item.barrier};
            if (item.asset) check(*kiyosi::make_asset_binary_barrier_option(terms));
            else check(*kiyosi::make_cash_binary_barrier_option(terms, 10.0));
        } else if (item.asset) {
            if (item.barrier == kiyosi::BarrierType::up_and_in)
                check(*kiyosi::make_asset_one_touch_up(expiry_date, expiry_date, item.level));
            else if (item.barrier == kiyosi::BarrierType::down_and_in)
                check(*kiyosi::make_asset_one_touch_down(expiry_date, expiry_date, item.level));
            else if (item.barrier == kiyosi::BarrierType::up_and_out)
                check(*kiyosi::make_asset_no_touch_up(expiry_date, expiry_date, item.level));
            else
                check(*kiyosi::make_asset_no_touch_down(expiry_date, expiry_date, item.level));
        } else {
            if (item.barrier == kiyosi::BarrierType::up_and_in)
                check(*kiyosi::make_cash_one_touch_up(expiry_date, expiry_date, item.level, 10.0));
            else if (item.barrier == kiyosi::BarrierType::down_and_in)
                check(*kiyosi::make_cash_one_touch_down(expiry_date, expiry_date, item.level, 10.0));
            else if (item.barrier == kiyosi::BarrierType::up_and_out)
                check(*kiyosi::make_cash_no_touch_up(expiry_date, expiry_date, item.level, 10.0));
            else
                check(*kiyosi::make_cash_no_touch_down(expiry_date, expiry_date, item.level, 10.0));
        }
    }
}

TEST_CASE("Scheduled binary barriers validate calendars and use the stored BGK interval")
{
    const auto short_schedule_checked = kiyosi::make_cash_no_touch_down(
        valuation, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled,
        {valuation + std::chrono::days{30}, valuation + std::chrono::days{60}, expiry_date});
    REQUIRE(short_schedule_checked);
    const auto& short_schedule = *short_schedule_checked;
    const auto long_schedule_checked = kiyosi::make_cash_no_touch_down(
        valuation, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled,
        {valuation + std::chrono::days{179}, expiry_date});
    REQUIRE(long_schedule_checked);
    const auto& long_schedule = *long_schedule_checked;
    const auto short_value_checked = kiyosi::AnalyticBinaryBarrierEngine{}.price(short_schedule, context());
    REQUIRE(short_value_checked);
    const auto short_value = *short_value_checked;
    const auto long_value_checked = kiyosi::AnalyticBinaryBarrierEngine{}.price(long_schedule, context());
    REQUIRE(long_value_checked);
    const auto long_value = *long_value_checked;
    CHECK(std::abs(short_value - long_value) > 1e-4);

    const auto weekend_checked = kiyosi::make_cash_no_touch_down(
        valuation, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled,
        {day(2025, 1, 11)});
    REQUIRE(weekend_checked);
    const auto& weekend = *weekend_checked;
    const auto parameters = kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    REQUIRE(parameters);
    const auto market_checked = kiyosi::make_pricing_context(*parameters,
                                                             100.0, valuation,
                                                             kiyosi::weekdays_calendar());
    REQUIRE(market_checked);
    const auto& market = *market_checked;
    const auto invalid_binary_date = kiyosi::AnalyticBinaryBarrierEngine{}.price(weekend, market);
    REQUIRE_FALSE(invalid_binary_date.has_value());
    CHECK(invalid_binary_date.error().category ==
          kiyosi::ErrorCategory::invalid_date);
}

TEST_CASE("Scheduled vanilla barriers validate events and refine")
{
    const std::vector<kiyosi::Date> observation_dates{
        valuation + std::chrono::days{37}, valuation + std::chrono::days{172}, expiry_date};
    const auto terms = kiyosi::BarrierOptionTerms{.option_type = kiyosi::OptionType::call,
                                                  .strike = 100.0,
                                                  .effective_date = valuation,
                                                  .expiry_date = expiry_date,
                                                  .barrier_level = 90.0,
                                                  .barrier_type = kiyosi::BarrierType::down_and_out,
                                                  .rebate = 2.0,
                                                  .rebate_timing = kiyosi::RebateTiming::at_expiry,
                                                  .observation_mode = kiyosi::ObservationMode::scheduled,
                                                  .observation_dates = observation_dates};
    const auto out_checked = kiyosi::make_barrier_option(terms);
    REQUIRE(out_checked);
    const auto& out = *out_checked;
    auto knock_in_terms = terms;
    knock_in_terms.barrier_type = kiyosi::BarrierType::down_and_in;
    const auto in_checked = kiyosi::make_barrier_option(knock_in_terms);
    REQUIRE(in_checked);
    const auto& in = *in_checked;
    const auto market = context();
    const auto coarse_checked = kiyosi::FiniteDifferenceBarrierEngine{80, 23}.price(out, market);
    REQUIRE(coarse_checked);
    const double coarse = *coarse_checked;
    const auto fine_checked = kiyosi::FiniteDifferenceBarrierEngine{240, 69}.price(out, market);
    REQUIRE(fine_checked);
    const double fine = *fine_checked;
    const auto analytic_checked = kiyosi::AnalyticBarrierEngine{}.price(out, market);
    REQUIRE(analytic_checked);
    const double analytic = *analytic_checked;
    CHECK(std::abs(fine - analytic) < std::abs(coarse - analytic));
    CHECK(*kiyosi::FiniteDifferenceBarrierEngine{240, 69}.price(in, market) > 0.0);

    auto weekend_terms = terms;
    weekend_terms.observation_dates = {day(2025, 1, 11)};
    const auto weekend_checked = kiyosi::make_barrier_option(weekend_terms);
    REQUIRE(weekend_checked);
    const auto& weekend = *weekend_checked;
    const auto parameters = kiyosi::make_bsm_parameters(0.04, 0.01, 0.3);
    REQUIRE(parameters);
    const auto weekdays_market_checked = kiyosi::make_pricing_context(
        *parameters, 100.0, valuation,
        kiyosi::weekdays_calendar());
    REQUIRE(weekdays_market_checked);
    const auto& weekdays_market = *weekdays_market_checked;
    const auto invalid_analytic_date = kiyosi::AnalyticBarrierEngine{}.price(weekend, weekdays_market);
    REQUIRE_FALSE(invalid_analytic_date.has_value());
    CHECK(invalid_analytic_date.error().category ==
          kiyosi::ErrorCategory::invalid_date);
    const auto invalid_fd_date = kiyosi::FiniteDifferenceBarrierEngine{}.price(weekend, weekdays_market);
    REQUIRE_FALSE(invalid_fd_date.has_value());
    CHECK(invalid_fd_date.error().category ==
          kiyosi::ErrorCategory::invalid_date);

    auto at_hit_terms = terms;
    at_hit_terms.rebate_timing = kiyosi::RebateTiming::at_hit;
    at_hit_terms.observation_dates = {valuation + std::chrono::days{37}, expiry_date};
    const auto at_hit_checked = kiyosi::make_barrier_option(at_hit_terms);
    REQUIRE(at_hit_checked);
    const auto& at_hit = *at_hit_checked;
    CHECK(*kiyosi::AnalyticBarrierEngine{}.price(at_hit, market) > 0.0);
}

TEST_CASE("Binomial and finite-difference prices converge toward analytic values")
{
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto american_call = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto market = context(100.0, 0.04, 0.0, 0.3);
    const double reference = *kiyosi::AnalyticVanillaEngine{}.price(option, market);

    std::array<double, 4> tree_errors{};
    for (std::size_t index = 0; index < tree_errors.size(); ++index) {
        const int steps = 64 << static_cast<int>(index);
        const auto result = kiyosi::CoxRossRubinsteinVanillaEngine{steps}.price(american_call, market);
        REQUIRE(result.has_value());
        tree_errors[index] = difference(*result, reference);
    }
    CHECK(tree_errors.back() < tree_errors.front());
    CHECK(tree_errors[2] < tree_errors[0]);
    CHECK(tree_errors.front() / tree_errors.back() > 1.5);

    std::array<double, 3> finite_difference_errors{};
    for (std::size_t index = 0; index < finite_difference_errors.size(); ++index) {
        // Keep the strike on a node so coarse-grid interpolation cannot cancel discretization error.
        const int steps = 64 << static_cast<int>(index);
        const auto result = kiyosi::FiniteDifferenceVanillaEngine{steps, steps}.price(option, market);
        REQUIRE(result.has_value());
        finite_difference_errors[index] = difference(*result, reference);
    }
    CHECK(finite_difference_errors.back() < finite_difference_errors.front());
    CHECK(finite_difference_errors[2] < finite_difference_errors[1]);
    CHECK(finite_difference_errors.front() / finite_difference_errors.back() > 1.5);
}

TEST_CASE("Explicit finite-difference engines honor signed stability grids")
{
    const auto grid_expiry = valuation + std::chrono::days{730};
    const auto call = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, grid_expiry);
    const auto digital = *kiyosi::make_cash_or_nothing_option(
        kiyosi::OptionType::call, 100.0, 10.0, valuation, grid_expiry);
    const auto barrier = *kiyosi::make_barrier_option({.option_type = kiyosi::OptionType::call,
                                                       .strike = 100.0,
                                                       .effective_date = valuation,
                                                       .expiry_date = grid_expiry,
                                                       .barrier_level = 90.0,
                                                       .barrier_type = kiyosi::BarrierType::down_and_out});

    for (const auto& [rate, volatility] : {
             std::tuple{0.75, 0.125}, std::tuple{0.0, 0.25}, std::tuple{-3.0, 0.5}}) {
        // Zero carry isolates the signed discount term from the drift stability bound.
        const auto market = context(100.0, rate, rate, volatility);
        const auto stable = kiyosi::FiniteDifferenceSettings{4, 100, kiyosi::FiniteDifferenceScheme::explicit_euler};
        const auto boundary = kiyosi::FiniteDifferenceSettings{4, 2, kiyosi::FiniteDifferenceScheme::explicit_euler};
        const auto unstable = kiyosi::FiniteDifferenceSettings{4, 1, kiyosi::FiniteDifferenceScheme::explicit_euler};

        const auto vanilla_stable = kiyosi::FiniteDifferenceVanillaEngine{stable}.price(call, market);
        CHECK(vanilla_stable.has_value());
        const auto vanilla_boundary = kiyosi::FiniteDifferenceVanillaEngine{boundary}.price(call, market);
        CHECK(vanilla_boundary.has_value());
        CHECK_FALSE(kiyosi::FiniteDifferenceVanillaEngine{unstable}.price(call, market).has_value());
        const auto digital_stable = kiyosi::FiniteDifferenceDigitalEngine{stable}.price(digital, market);
        CHECK(digital_stable.has_value());
        const auto digital_boundary = kiyosi::FiniteDifferenceDigitalEngine{boundary}.price(digital, market);
        CHECK(digital_boundary.has_value());
        CHECK_FALSE(kiyosi::FiniteDifferenceDigitalEngine{unstable}.price(digital, market).has_value());
        const auto barrier_stable = kiyosi::FiniteDifferenceBarrierEngine{stable}.price(barrier, market);
        CHECK(barrier_stable.has_value());
        const auto barrier_boundary = kiyosi::FiniteDifferenceBarrierEngine{boundary}.price(barrier, market);
        CHECK(barrier_boundary.has_value());
        CHECK_FALSE(kiyosi::FiniteDifferenceBarrierEngine{unstable}.price(barrier, market).has_value());
    }
}

} // namespace
