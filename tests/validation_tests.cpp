#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <tuple>
#include <vector>

#include <kiyosi/kiyosi.hpp>

namespace {

kiyosi::date day(int year, unsigned month, unsigned day_number)
{
    return kiyosi::date{std::chrono::year{year} / std::chrono::month{month} / std::chrono::day{day_number}};
}

const auto valuation = day(2025, 1, 6);
const auto expiry = valuation + std::chrono::days{365};

kiyosi::PricingContext context(double spot = 100.0, double rate = 0.04,
                               double dividend = 0.01, double volatility = 0.3,
                               kiyosi::date value_date = valuation)
{
    const auto parameters = *kiyosi::make_bsm_parameters(rate, dividend, volatility);
    return *kiyosi::make_pricing_context(parameters, *kiyosi::make_asset_price(spot), value_date);
}

kiyosi::PricingResult analytic(kiyosi::option_type type, double spot = 100.0, double rate = 0.04,
                               double dividend = 0.01, double volatility = 0.3,
                               kiyosi::date value_date = valuation, kiyosi::date option_expiry = expiry,
                               double strike = 100.0)
{
    const auto option = *kiyosi::make_european_option(type, strike, option_expiry);
    return *kiyosi::AnalyticEuropeanEngine{}.price(
        option, context(spot, rate, dividend, volatility, value_date));
}

double difference(double left, double right)
{
    return std::abs(left - right);
}

double risk_value(const kiyosi::PricingResult& result, kiyosi::risk_measure measure)
{
    return *result.get(measure);
}

void check_close(double actual, double expected, double absolute = 1e-7, double relative = 1e-5)
{
    CHECK(difference(actual, expected) <= absolute + relative * std::abs(expected));
}

double value(kiyosi::option_type type, double spot, double rate, double dividend,
             double volatility, kiyosi::date value_date, kiyosi::date option_expiry, double strike = 100.0)
{
    return risk_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                      kiyosi::risk_measure::price);
}

double delta(kiyosi::option_type type, double spot, double rate, double dividend,
             double volatility, kiyosi::date value_date, kiyosi::date option_expiry, double strike = 100.0)
{
    return risk_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                      kiyosi::risk_measure::delta);
}

double gamma(kiyosi::option_type type, double spot, double rate, double dividend,
             double volatility, kiyosi::date value_date, kiyosi::date option_expiry, double strike = 100.0)
{
    return risk_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                      kiyosi::risk_measure::gamma);
}

TEST_CASE("Analytic Greeks agree with central finite differences")
{
    const double spot_step = 0.01;
    const double volatility_step = 1e-4;
    const auto previous_day = valuation - std::chrono::days{1};
    const auto next_day = valuation + std::chrono::days{1};

    for (const auto type : {kiyosi::option_type::call, kiyosi::option_type::put}) {

        const double center = value(type, 100.0, 0.04, 0.01, 0.3, valuation, expiry);
        const double up = value(type, 100.0 + spot_step, 0.04, 0.01, 0.3, valuation, expiry);
        const double down = value(type, 100.0 - spot_step, 0.04, 0.01, 0.3, valuation, expiry);
        const double up_two = value(type, 100.0 + 2.0 * spot_step, 0.04, 0.01, 0.3, valuation, expiry);
        const double down_two = value(type, 100.0 - 2.0 * spot_step, 0.04, 0.01, 0.3, valuation, expiry);
        const double delta_fd = (up - down) / (2.0 * spot_step);
        const double gamma_fd = (up - 2.0 * center + down) / (spot_step * spot_step);
        const double speed_fd = (up_two - 2.0 * up + 2.0 * down - down_two) /
                                (2.0 * spot_step * spot_step * spot_step);

        const auto result = analytic(type);
        check_close(risk_value(result, kiyosi::risk_measure::delta), delta_fd, 2e-7, 2e-4);
        check_close(risk_value(result, kiyosi::risk_measure::gamma), gamma_fd, 2e-7, 2e-4);
        check_close(risk_value(result, kiyosi::risk_measure::speed), speed_fd, 2e-6, 2e-3);

        const double theta_fd = (value(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry) -
                                 value(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry)) /
                                2.0;
        const double charm_fd = (delta(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry) -
                                 delta(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry)) /
                                2.0;
        const double color_fd = (gamma(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry) -
                                 gamma(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry)) /
                                2.0;
        check_close(risk_value(result, kiyosi::risk_measure::theta), theta_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::risk_measure::charm), charm_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::risk_measure::color), color_fd, 2e-6, 2e-3);

        const double vega_fd = (value(type, 100.0, 0.04, 0.01, 0.3 + volatility_step, valuation, expiry) -
                                value(type, 100.0, 0.04, 0.01, 0.3 - volatility_step, valuation, expiry)) /
                               (2.0 * volatility_step * 100.0);
        const double vanna_fd = (delta(type, 100.0, 0.04, 0.01, 0.3 + volatility_step, valuation, expiry) -
                                 delta(type, 100.0, 0.04, 0.01, 0.3 - volatility_step, valuation, expiry)) /
                                (2.0 * volatility_step * 100.0);
        const double zomma_fd = (gamma(type, 100.0, 0.04, 0.01, 0.3 + volatility_step, valuation, expiry) -
                                 gamma(type, 100.0, 0.04, 0.01, 0.3 - volatility_step, valuation, expiry)) /
                                (2.0 * volatility_step * 100.0);
        const double rho_step = 1e-4;
        const double rho_fd = (value(type, 100.0, 0.04 + rho_step, 0.01, 0.3, valuation, expiry) -
                               value(type, 100.0, 0.04 - rho_step, 0.01, 0.3, valuation, expiry)) /
                              (2.0 * rho_step * 100.0);
        check_close(risk_value(result, kiyosi::risk_measure::vega), vega_fd, 2e-6, 2e-4);
        check_close(risk_value(result, kiyosi::risk_measure::vanna), vanna_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::risk_measure::zomma), zomma_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::risk_measure::rho), rho_fd, 2e-6, 2e-4);
    }
}

TEST_CASE("Analytic pricing satisfies no-arbitrage identities")
{
    const auto call = analytic(kiyosi::option_type::call);
    const auto put = analytic(kiyosi::option_type::put);
    const double time = 1.0;
    check_close(risk_value(call, kiyosi::risk_measure::price) - risk_value(put, kiyosi::risk_measure::price), 100.0 * std::exp(-0.01 * time) - 100.0 * std::exp(-0.04 * time), 1e-10, 1e-10);
    check_close(risk_value(call, kiyosi::risk_measure::delta) - risk_value(put, kiyosi::risk_measure::delta), std::exp(-0.01 * time), 1e-10, 1e-10);
    check_close(risk_value(call, kiyosi::risk_measure::gamma), risk_value(put, kiyosi::risk_measure::gamma), 1e-10, 1e-10);
    check_close(risk_value(call, kiyosi::risk_measure::vega), risk_value(put, kiyosi::risk_measure::vega), 1e-10, 1e-10);

    const kiyosi::AnalyticDigitalEngine digital;
    for (const auto type : {kiyosi::option_type::call, kiyosi::option_type::put}) {
        const auto cash = *kiyosi::make_cash_or_nothing_option(type, 100.0, 100.0, expiry);
        const auto asset = *kiyosi::make_asset_or_nothing_option(type, 100.0, expiry);
        const auto vanilla = analytic(type);
        const auto cash_value = risk_value(*digital.price(cash, context()), kiyosi::risk_measure::price);
        const auto asset_value = risk_value(*digital.price(asset, context()), kiyosi::risk_measure::price);
        check_close(type == kiyosi::option_type::call ? asset_value - cash_value : cash_value - asset_value,
                    risk_value(vanilla, kiyosi::risk_measure::price), 2e-10, 2e-10);
    }

    const kiyosi::AnalyticBarrierEngine barriers;
    for (const auto kind : {kiyosi::barrier_type::up_and_in, kiyosi::barrier_type::up_and_out,
                            kiyosi::barrier_type::down_and_in, kiyosi::barrier_type::down_and_out}) {
        const auto option = *kiyosi::make_barrier_option(
            kiyosi::option_type::call, 100.0, expiry,
            kind == kiyosi::barrier_type::up_and_in || kind == kiyosi::barrier_type::up_and_out ? 130.0 : 75.0,
            kind);
        const auto paired_kind = kind == kiyosi::barrier_type::up_and_in
                                     ? kiyosi::barrier_type::up_and_out
                                 : kind == kiyosi::barrier_type::up_and_out  ? kiyosi::barrier_type::up_and_in
                                 : kind == kiyosi::barrier_type::down_and_in ? kiyosi::barrier_type::down_and_out
                                                                             : kiyosi::barrier_type::down_and_in;
        const auto paired = *kiyosi::make_barrier_option(
            kiyosi::option_type::call, 100.0, expiry,
            kind == kiyosi::barrier_type::up_and_in || kind == kiyosi::barrier_type::up_and_out ? 130.0 : 75.0,
            paired_kind);
        check_close(risk_value(*barriers.price(option, context()), kiyosi::risk_measure::price) +
                        risk_value(*barriers.price(paired, context()), kiyosi::risk_measure::price),
                    risk_value(analytic(kiyosi::option_type::call), kiyosi::risk_measure::price), 2e-5, 2e-5);
    }
}

TEST_CASE("Analytic prices are monotone and bounded")
{
    for (const auto type : {kiyosi::option_type::call, kiyosi::option_type::put}) {
        const double short_value = value(type, 100.0, 0.0, 0.0, 0.2, valuation,
                                         valuation + std::chrono::days{182});
        const double long_value = value(type, 100.0, 0.0, 0.0, 0.2, valuation,
                                        valuation + std::chrono::days{730});
        CHECK(long_value >= short_value);
        const double low_volatility = value(type, 100.0, 0.0, 0.0, 0.1, valuation, expiry);
        const double high_volatility = value(type, 100.0, 0.0, 0.0, 0.4, valuation, expiry);
        CHECK(high_volatility >= low_volatility);

        const double price = value(type, 110.0, 0.0, 0.0, 0.2, valuation, expiry);
        const double intrinsic = type == kiyosi::option_type::call ? 10.0 : 0.0;
        const double upper_bound = type == kiyosi::option_type::call ? 110.0 : 100.0;
        CHECK(intrinsic <= price);
        CHECK(price <= upper_bound);
    }
}

TEST_CASE("Analytic binary barriers match the pinned DerivaSharp matrix")
{
    struct binary_case { bool asset; kiyosi::barrier_type barrier; kiyosi::rebate_timing timing; std::optional<kiyosi::option_type> type; double level; double payout; double expected; };
    const std::array<binary_case, 28> cases{
        binary_case{false, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_hit, {}, 90, 10, 7.310536},
        {false, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_hit, {}, 110, 10, 7.322345},
        {true, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_hit, {}, 90, 90, 65.794826},
        {true, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_hit, {}, 110, 110, 80.545795},
        {false, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_expiry, {}, 90, 10, 7.091270},
        {false, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_expiry, {}, 110, 10, 7.097140},
        {true, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_expiry, {}, 90, 0, 65.295307},
        {true, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_expiry, {}, 110, 0, 79.918599},
        {false, kiyosi::barrier_type::down_and_out, kiyosi::rebate_timing::at_expiry, {}, 90, 10, 2.516625},
        {false, kiyosi::barrier_type::up_and_out, kiyosi::rebate_timing::at_expiry, {}, 110, 10, 2.510755},
        {true, kiyosi::barrier_type::down_and_out, kiyosi::rebate_timing::at_expiry, {}, 90, 0, 33.709677},
        {true, kiyosi::barrier_type::up_and_out, kiyosi::rebate_timing::at_expiry, {}, 110, 0, 19.086385},
        {false, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 90, 10, 2.248046},
        {false, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 110, 10, 4.499068},
        {true, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 90, 0, 27.035296},
        {true, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 110, 0, 58.104678},
        {false, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 90, 10, 4.843224},
        {false, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 110, 10, 2.598072},
        {true, kiyosi::barrier_type::down_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 90, 0, 38.260011},
        {true, kiyosi::barrier_type::up_and_in, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 110, 0, 21.813921},
        {false, kiyosi::barrier_type::down_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 90, 10, 2.364332},
        {false, kiyosi::barrier_type::up_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 110, 10, 0.113309},
        {true, kiyosi::barrier_type::down_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 90, 0, 32.239613},
        {true, kiyosi::barrier_type::up_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::call, 110, 0, 1.170232},
        {false, kiyosi::barrier_type::down_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 90, 10, 0.152293},
        {false, kiyosi::barrier_type::up_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 110, 10, 2.397445},
        {true, kiyosi::barrier_type::down_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 90, 0, 1.470063},
        {true, kiyosi::barrier_type::up_and_out, kiyosi::rebate_timing::at_expiry, kiyosi::option_type::put, 110, 0, 17.916153},
    };
    for (const auto& item : cases) {
        const auto option = kiyosi::make_binary_barrier_option(item.type, 100.0, valuation, expiry, item.level,
                                                                item.barrier, item.payout, item.asset, item.timing);
        REQUIRE(option.has_value());
        const auto result = kiyosi::AnalyticBinaryBarrierEngine{}.price(*option, context());
        REQUIRE(result.has_value());
        check_close(risk_value(*result, kiyosi::risk_measure::price), item.expected, 5e-7, 0.0);
    }
}

TEST_CASE("Binary barrier expiry uses inclusive hits and strict strikes")
{
    struct expiry_case { bool asset; kiyosi::barrier_type barrier; std::optional<kiyosi::option_type> type; double strike; double level; double expected; };
    const std::array<expiry_case, 8> cases{
        expiry_case{false, kiyosi::barrier_type::up_and_in, kiyosi::option_type::call, 100, 100, 0},
        {false, kiyosi::barrier_type::down_and_in, kiyosi::option_type::put, 101, 100, 10},
        {true, kiyosi::barrier_type::up_and_in, {}, 100, 100, 100},
        {true, kiyosi::barrier_type::down_and_in, kiyosi::option_type::put, 101, 100, 100},
        {false, kiyosi::barrier_type::up_and_out, {}, 100, 100, 0},
        {true, kiyosi::barrier_type::down_and_out, {}, 100, 100, 0},
        {false, kiyosi::barrier_type::up_and_out, {}, 100, 110, 10},
        {true, kiyosi::barrier_type::down_and_out, kiyosi::option_type::call, 99, 90, 100},
    };
    for (const auto& item : cases) {
        const auto option = *kiyosi::make_binary_barrier_option(
            item.type, item.strike, expiry, expiry, item.level, item.barrier, item.asset ? 0.0 : 10.0, item.asset);
        CHECK(risk_value(*kiyosi::AnalyticBinaryBarrierEngine{}.price(option, context(100.0, 0.04, 0.01, 0.3, expiry)),
                         kiyosi::risk_measure::price) == item.expected);
    }
}

TEST_CASE("Scheduled binary barriers validate calendars and use the stored BGK interval")
{
    const auto short_schedule = *kiyosi::make_binary_barrier_option(
        std::nullopt, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out, 10.0, false,
        kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled,
        std::vector<kiyosi::date>{valuation + std::chrono::days{30}, valuation + std::chrono::days{60}});
    const auto long_schedule = *kiyosi::make_binary_barrier_option(
        std::nullopt, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out, 10.0, false,
        kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled,
        std::vector<kiyosi::date>{valuation + std::chrono::days{180}, expiry});
    const auto short_value = risk_value(*kiyosi::AnalyticBinaryBarrierEngine{}.price(short_schedule, context()), kiyosi::risk_measure::price);
    const auto long_value = risk_value(*kiyosi::AnalyticBinaryBarrierEngine{}.price(long_schedule, context()), kiyosi::risk_measure::price);
    check_close(short_value, 3.651891897184211, 1e-12, 0.0);
    CHECK(std::abs(short_value - long_value) > 1e-4);

    const auto weekend = *kiyosi::make_binary_barrier_option(
        std::nullopt, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out, 10.0, false,
        kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled,
        std::vector<kiyosi::date>{day(2025, 1, 11)});
    const auto market = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.04, 0.01, 0.3),
                                                       *kiyosi::make_asset_price(100.0), valuation,
                                                       kiyosi::exchange_calendar());
    CHECK(kiyosi::AnalyticBinaryBarrierEngine{}.price(weekend, market).error().category ==
          kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Scheduled vanilla barriers validate events and refine")
{
    const std::vector<kiyosi::date> observations{
        valuation + std::chrono::days{37}, valuation + std::chrono::days{173}, expiry};
    const auto out = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out,
        2.0, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled, observations);
    const auto in = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_in,
        2.0, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled, observations);
    const auto market = context();
    const double coarse = risk_value(*kiyosi::FiniteDifferenceBarrierEngine{80, 23}.price(out, market), kiyosi::risk_measure::price);
    const double fine = risk_value(*kiyosi::FiniteDifferenceBarrierEngine{240, 69}.price(out, market), kiyosi::risk_measure::price);
    const double analytic = risk_value(*kiyosi::AnalyticBarrierEngine{}.price(out, market), kiyosi::risk_measure::price);
    CHECK(std::abs(fine - analytic) < std::abs(coarse - analytic));
    CHECK(risk_value(*kiyosi::FiniteDifferenceBarrierEngine{240, 69}.price(in, market), kiyosi::risk_measure::price) > 0.0);

    const auto weekend = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out,
        2.0, kiyosi::rebate_timing::at_expiry, kiyosi::observation_mode::scheduled,
        std::vector<kiyosi::date>{day(2025, 1, 11)});
    const auto exchange_market = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), *kiyosi::make_asset_price(100.0), valuation,
        kiyosi::exchange_calendar());
    CHECK(kiyosi::AnalyticBarrierEngine{}.price(weekend, exchange_market).error().category ==
          kiyosi::error_category::invalid_schedule);
    CHECK(kiyosi::FiniteDifferenceBarrierEngine{}.price(weekend, exchange_market).error().category ==
          kiyosi::error_category::invalid_date);

    const auto at_hit = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, valuation, expiry, 90.0, kiyosi::barrier_type::down_and_out,
        2.0, kiyosi::rebate_timing::at_hit, kiyosi::observation_mode::scheduled,
        std::vector<kiyosi::date>{valuation + std::chrono::days{37}, expiry});
    CHECK(risk_value(*kiyosi::AnalyticBarrierEngine{}.price(at_hit, market), kiyosi::risk_measure::price) > 0.0);
}

TEST_CASE("Haug and Hull Black-Scholes reference values remain fixed")
{
    struct case_data {
        double spot;
        double strike;
        double rate;
        double dividend;
        double volatility;
        int days;
        double call;
        double put;
    };
    const std::array cases{
        case_data{100.0, 100.0, 0.10, 0.0, 0.20, 365, 13.2696765847, 3.7534183883},
        case_data{50.0, 50.0, 0.10, 0.0, 0.30, 182, 5.4441844402, 3.0121713816},
        case_data{42.0, 40.0, 0.10, 0.0, 0.20, 182, 4.7531749689, 0.8075645220},
        case_data{100.0, 110.0, 0.05, 0.02, 0.25, 730, 12.0647830432, 15.5179551119},
    };
    for (const auto& item : cases) {
        const auto value_date = valuation;
        const auto option_expiry = value_date + std::chrono::days{item.days};
        check_close(value(kiyosi::option_type::call, item.spot, item.rate, item.dividend,
                          item.volatility, value_date, option_expiry, item.strike),
                    item.call, 2e-8, 2e-8);
        check_close(value(kiyosi::option_type::put, item.spot, item.rate, item.dividend,
                          item.volatility, value_date, option_expiry, item.strike),
                    item.put, 2e-8, 2e-8);
    }
}

TEST_CASE("Binomial and finite-difference prices converge toward analytic values")
{
    const auto option = *kiyosi::make_european_call(100.0, expiry);
    const auto american_call = *kiyosi::make_american_call(100.0, expiry);
    const auto market = context(100.0, 0.04, 0.0, 0.3);
    const double reference = risk_value(*kiyosi::AnalyticEuropeanEngine{}.price(option, market), kiyosi::risk_measure::price);

    std::array<double, 4> tree_errors{};
    for (std::size_t index = 0; index < tree_errors.size(); ++index) {
        const int steps = 64 << static_cast<int>(index);
        const auto result = kiyosi::BinomialAmericanEngine{steps}.price(american_call, market);
        REQUIRE(result.has_value());
        tree_errors[index] = difference(risk_value(*result, kiyosi::risk_measure::price), reference);
    }
    CHECK(tree_errors.back() < tree_errors.front());
    CHECK(tree_errors[2] < tree_errors[0]);
    CHECK(tree_errors.front() / tree_errors.back() > 1.5);

    std::array<double, 3> finite_difference_errors{};
    for (std::size_t index = 0; index < finite_difference_errors.size(); ++index) {
        const int steps = 50 << static_cast<int>(index);
        const auto result = kiyosi::FiniteDifferenceEuropeanEngine{steps, steps}.price(option, market);
        REQUIRE(result.has_value());
        finite_difference_errors[index] = difference(risk_value(*result, kiyosi::risk_measure::price), reference);
    }
    CHECK(finite_difference_errors.back() < finite_difference_errors.front());
    CHECK(finite_difference_errors[2] < finite_difference_errors[1]);
    CHECK(finite_difference_errors.front() / finite_difference_errors.back() > 1.5);
}

TEST_CASE("Explicit finite-difference engines honor signed stability grids")
{
    const auto grid_expiry = valuation + std::chrono::days{730};
    const auto call = *kiyosi::make_european_call(100.0, grid_expiry);
    const auto digital = *kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 10.0, grid_expiry);
    const auto barrier = *kiyosi::make_barrier_option(
        kiyosi::option_type::call, 100.0, valuation, grid_expiry, 90.0, kiyosi::barrier_type::down_and_out);

    for (const auto [rate, volatility] : {
             std::tuple{0.75, 0.125}, std::tuple{0.0, 0.25}, std::tuple{-3.0, 0.5}}) {
        const auto market = context(100.0, rate, 0.01, volatility);
        const auto stable = kiyosi::FiniteDifferenceSettings{4, 100, kiyosi::finite_difference_scheme::explicit_euler};
        const auto boundary = kiyosi::FiniteDifferenceSettings{4, 2, kiyosi::finite_difference_scheme::explicit_euler};
        const auto unstable = kiyosi::FiniteDifferenceSettings{4, 1, kiyosi::finite_difference_scheme::explicit_euler};

        const auto vanilla_stable = kiyosi::FiniteDifferenceEuropeanEngine{stable}.price(call, market);
        CHECK(vanilla_stable.has_value());
        const auto vanilla_boundary = kiyosi::FiniteDifferenceEuropeanEngine{boundary}.price(call, market);
        CHECK((vanilla_boundary || vanilla_boundary.error().message != "explicit finite-difference grid is unstable"));
        CHECK_FALSE(kiyosi::FiniteDifferenceEuropeanEngine{unstable}.price(call, market).has_value());
        const auto digital_stable = kiyosi::FiniteDifferenceDigitalEngine{stable}.price(digital, market);
        CHECK(digital_stable.has_value());
        const auto digital_boundary = kiyosi::FiniteDifferenceDigitalEngine{boundary}.price(digital, market);
        CHECK((digital_boundary || digital_boundary.error().message != "explicit finite-difference grid is unstable"));
        CHECK_FALSE(kiyosi::FiniteDifferenceDigitalEngine{unstable}.price(digital, market).has_value());
        const auto barrier_stable = kiyosi::FiniteDifferenceBarrierEngine{stable}.price(barrier, market);
        CHECK(barrier_stable.has_value());
        const auto barrier_boundary = kiyosi::FiniteDifferenceBarrierEngine{boundary}.price(barrier, market);
        CHECK((barrier_boundary || barrier_boundary.error().message != "explicit finite-difference grid is unstable"));
        CHECK_FALSE(kiyosi::FiniteDifferenceBarrierEngine{unstable}.price(barrier, market).has_value());
    }
}

TEST_CASE("Exercise-based options compose shared terms, payoff, and exercise")
{
    const auto terms = *kiyosi::make_option_terms(kiyosi::option_type::call, 100.0, expiry);
    const auto payoff = *kiyosi::make_cash_or_nothing_payoff(10.0);
    const auto european = kiyosi::make_european_option(terms, payoff);
    REQUIRE(european.has_value());
    CHECK(european->type() == kiyosi::option_type::call);
    CHECK(european->strike() == 100.0);
    CHECK(european->payout() == 10.0);
    CHECK(european->exercise() == kiyosi::EuropeanExercise{});

    const auto dates = std::vector{valuation + std::chrono::days{30}, valuation + std::chrono::days{180}};
    const auto bermudan = kiyosi::make_bermudan_option(terms, kiyosi::VanillaPayoff{}, dates);
    REQUIRE(bermudan.has_value());
    CHECK(bermudan->exercise_dates() == dates);
    CHECK_FALSE(kiyosi::make_bermudan_option(terms, kiyosi::VanillaPayoff{},
                                             std::vector<kiyosi::date>{expiry + std::chrono::days{1}})
                    .has_value());

    const auto invalid_type = static_cast<kiyosi::option_type>(99);
    for (const auto invalid : {
             kiyosi::make_european_option(invalid_type, 100.0, expiry).error().category,
             kiyosi::make_cash_or_nothing_option(invalid_type, 100.0, 10.0, expiry).error().category,
             kiyosi::make_asset_or_nothing_option(invalid_type, 100.0, expiry).error().category}) {
        CHECK(invalid == kiyosi::error_category::invalid_option);
    }
    CHECK(kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 0.0, 10.0, expiry)
              .error()
              .category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_asset_or_nothing_option(kiyosi::option_type::call, 0.0, expiry)
              .error()
              .category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 0.0, expiry)
              .error()
              .category == kiyosi::error_category::invalid_parameter);

    CHECK(kiyosi::make_bermudan_option(terms, kiyosi::VanillaPayoff{}, {}).error().category ==
          kiyosi::error_category::invalid_schedule);
    CHECK(kiyosi::make_bermudan_option(
              terms, kiyosi::VanillaPayoff{},
              std::vector{valuation + std::chrono::days{30}, valuation + std::chrono::days{30}})
              .error()
              .category == kiyosi::error_category::invalid_schedule);
    CHECK(kiyosi::make_bermudan_option(
              terms, kiyosi::VanillaPayoff{}, std::vector{day(2025, 1, 11)}, kiyosi::exchange_calendar())
              .error()
              .category == kiyosi::error_category::invalid_schedule);

    const auto later_terms = *kiyosi::make_option_terms(
        kiyosi::option_type::call, 100.0, expiry + std::chrono::days{30});
    const auto later_exercise = *kiyosi::make_bermudan_exercise(
        std::vector{expiry + std::chrono::days{1}}, later_terms.expiry());
    CHECK(kiyosi::make_exercise_based_option(terms, kiyosi::VanillaPayoff{}, later_exercise)
              .error()
              .category == kiyosi::error_category::invalid_schedule);
}

} // namespace
