#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
#include <tuple>
#include <vector>

#include <kiyosi/kiyosi.hpp>

namespace {

kiyosi::Date day(int year, unsigned month, unsigned day_number)
{
    return kiyosi::Date{std::chrono::year{year} / std::chrono::month{month} / std::chrono::day{day_number}};
}

const auto valuation = day(2025, 1, 6);
const auto expiry_date = valuation + std::chrono::days{365};

kiyosi::PricingContext context(double spot = 100.0, double rate = 0.04,
                               double dividend = 0.01, double volatility = 0.3,
                               kiyosi::Date value_date = valuation)
{
    const auto parameters = *kiyosi::make_bsm_parameters(rate, dividend, volatility);
    return *kiyosi::make_pricing_context(parameters, spot, value_date);
}

kiyosi::PricingResult analytic(kiyosi::OptionType type, double spot = 100.0, double rate = 0.04,
                               double dividend = 0.01, double volatility = 0.3,
                               kiyosi::Date value_date = valuation, kiyosi::Date option_expiry = expiry_date,
                               double strike = 100.0)
{
    const auto option = *kiyosi::make_european_option(type, strike, value_date, option_expiry);
    return *kiyosi::AnalyticVanillaEngine{}.price(
        option, context(spot, rate, dividend, volatility, value_date));
}

double difference(double left, double right)
{
    return std::abs(left - right);
}

double risk_value(const kiyosi::PricingResult& result, kiyosi::RiskMeasure measure)
{
    return *result.require(measure);
}

void check_close(double actual, double expected, double absolute = 1e-7, double relative = 1e-5)
{
    CHECK(difference(actual, expected) <= absolute + relative * std::abs(expected));
}

double value(kiyosi::OptionType type, double spot, double rate, double dividend,
             double volatility, kiyosi::Date value_date, kiyosi::Date option_expiry, double strike = 100.0)
{
    return risk_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                      kiyosi::RiskMeasure::price);
}

double delta(kiyosi::OptionType type, double spot, double rate, double dividend,
             double volatility, kiyosi::Date value_date, kiyosi::Date option_expiry, double strike = 100.0)
{
    return risk_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                      kiyosi::RiskMeasure::delta);
}

double gamma(kiyosi::OptionType type, double spot, double rate, double dividend,
             double volatility, kiyosi::Date value_date, kiyosi::Date option_expiry, double strike = 100.0)
{
    return risk_value(analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike),
                      kiyosi::RiskMeasure::gamma);
}

struct RejectingMixedBumpEngine {
    kiyosi::Result<kiyosi::PricingResult> price(
        const kiyosi::EuropeanOption&, const kiyosi::PricingContext& market) const
    {
        if (market.spot_price() > 100.0 && market.model_parameters().volatility() > 0.3)
            return std::unexpected(kiyosi::Error{
                kiyosi::ErrorCategory::invalid_schedule, "feasible mixed bump rejected"});
        return kiyosi::make_pricing_result(
            {{kiyosi::RiskMeasure::price,
              market.spot_price() + market.model_parameters().volatility()}});
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
        check_close(risk_value(result, kiyosi::RiskMeasure::delta), delta_fd, 2e-7, 2e-4);
        check_close(risk_value(result, kiyosi::RiskMeasure::gamma), gamma_fd, 2e-7, 2e-4);
        check_close(risk_value(result, kiyosi::RiskMeasure::speed), speed_fd, 2e-6, 2e-3);

        const double theta_fd = (value(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry_date) -
                                 value(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry_date)) /
                                2.0;
        const double charm_fd = (delta(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry_date) -
                                 delta(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry_date)) /
                                2.0;
        const double color_fd = (gamma(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry_date) -
                                 gamma(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry_date)) /
                                2.0;
        check_close(risk_value(result, kiyosi::RiskMeasure::theta), theta_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::RiskMeasure::charm), charm_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::RiskMeasure::color), color_fd, 2e-6, 2e-3);

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
        check_close(risk_value(result, kiyosi::RiskMeasure::vega), vega_fd, 2e-6, 2e-4);
        check_close(risk_value(result, kiyosi::RiskMeasure::vanna), vanna_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::RiskMeasure::zomma), zomma_fd, 2e-6, 2e-3);
        check_close(risk_value(result, kiyosi::RiskMeasure::rho), rho_fd, 2e-6, 2e-4);
    }
}

TEST_CASE("Analytic and numerical analytics share risk-measure conventions")
{
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation - std::chrono::days{30}, expiry_date);
    const auto market = context();
    const kiyosi::AnalyticVanillaEngine engine;
    const auto analytic_result = *engine.price(option, market);
    const auto numerical_result = *kiyosi::calculate_numerical_risk_measures(engine, option, market);

    for (std::size_t index = 0; index < kiyosi::risk_measure_count; ++index) {
        const auto measure = static_cast<kiyosi::RiskMeasure>(index);
        INFO("risk measure index: " << index);
        check_close(risk_value(numerical_result, measure), risk_value(analytic_result, measure),
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
        const auto result = kiyosi::calculate_numerical_risk_measures(engine, option, market);

        REQUIRE(result);
        check_close(risk_value(*result, kiyosi::RiskMeasure::price),
                    risk_value(direct, kiyosi::RiskMeasure::price));
        CHECK(result->has(kiyosi::RiskMeasure::delta));
        CHECK(result->has(kiyosi::RiskMeasure::rho));
        CHECK_FALSE(result->has(kiyosi::RiskMeasure::vega));
        CHECK_FALSE(result->has(kiyosi::RiskMeasure::vanna));
        CHECK_FALSE(result->has(kiyosi::RiskMeasure::zomma));
    }

    SECTION("low spot")
    {
        const auto market = context(0.005);
        const auto direct = *engine.price(option, market);
        const auto result = kiyosi::calculate_numerical_risk_measures(engine, option, market);

        REQUIRE(result);
        check_close(risk_value(*result, kiyosi::RiskMeasure::price),
                    risk_value(direct, kiyosi::RiskMeasure::price));
        CHECK(result->has(kiyosi::RiskMeasure::vega));
        CHECK(result->has(kiyosi::RiskMeasure::theta));
        CHECK(result->has(kiyosi::RiskMeasure::rho));
        for (const auto measure : {
                 kiyosi::RiskMeasure::delta, kiyosi::RiskMeasure::gamma,
                 kiyosi::RiskMeasure::speed, kiyosi::RiskMeasure::charm,
                 kiyosi::RiskMeasure::color, kiyosi::RiskMeasure::vanna,
                 kiyosi::RiskMeasure::zomma})
            CHECK_FALSE(result->has(measure));
    }

    SECTION("no time direction")
    {
        const auto expiring = *kiyosi::make_european_option(
            kiyosi::OptionType::call, 100.0, valuation, valuation);
        const auto result = kiyosi::calculate_numerical_risk_measures(engine, expiring, context());

        REQUIRE(result);
        CHECK(result->has(kiyosi::RiskMeasure::price));
        CHECK_FALSE(result->has(kiyosi::RiskMeasure::theta));
        CHECK_FALSE(result->has(kiyosi::RiskMeasure::charm));
        CHECK_FALSE(result->has(kiyosi::RiskMeasure::color));
    }
}

TEST_CASE("Numerical analytics preserve feasible bump failures")
{
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto result = kiyosi::calculate_numerical_risk_measures(
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
    check_close(risk_value(call, kiyosi::RiskMeasure::price) - risk_value(put, kiyosi::RiskMeasure::price), 100.0 * std::exp(-0.01 * time) - 100.0 * std::exp(-0.04 * time), 1e-10, 1e-10);
    check_close(risk_value(call, kiyosi::RiskMeasure::delta) - risk_value(put, kiyosi::RiskMeasure::delta), std::exp(-0.01 * time), 1e-10, 1e-10);
    check_close(risk_value(call, kiyosi::RiskMeasure::gamma), risk_value(put, kiyosi::RiskMeasure::gamma), 1e-10, 1e-10);
    check_close(risk_value(call, kiyosi::RiskMeasure::vega), risk_value(put, kiyosi::RiskMeasure::vega), 1e-10, 1e-10);

    const kiyosi::AnalyticDigitalEngine digital;
    for (const auto type : {kiyosi::OptionType::call, kiyosi::OptionType::put}) {
        const auto cash = *kiyosi::make_cash_or_nothing_option(type, 100.0, 100.0, valuation, expiry_date);
        const auto asset = *kiyosi::make_asset_or_nothing_option(type, 100.0, valuation, expiry_date);
        const auto vanilla = analytic(type);
        const auto cash_value = risk_value(*digital.price(cash, context()), kiyosi::RiskMeasure::price);
        const auto asset_value = risk_value(*digital.price(asset, context()), kiyosi::RiskMeasure::price);
        check_close(type == kiyosi::OptionType::call ? asset_value - cash_value : cash_value - asset_value,
                    risk_value(vanilla, kiyosi::RiskMeasure::price), 2e-10, 2e-10);
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
        check_close(risk_value(*barriers.price(option, context()), kiyosi::RiskMeasure::price) +
                        risk_value(*barriers.price(paired, context()), kiyosi::RiskMeasure::price),
                    risk_value(analytic(kiyosi::OptionType::call), kiyosi::RiskMeasure::price), 2e-5, 2e-5);
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
    struct expiry_case {
        bool asset;
        kiyosi::BarrierType barrier;
        std::optional<kiyosi::OptionType> type;
        double strike;
        double level;
        double expected;
    };
    const std::array<expiry_case, 8> cases{
        expiry_case{false, kiyosi::BarrierType::up_and_in, kiyosi::OptionType::call, 100, 100, 0},
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
            CHECK(risk_value(*kiyosi::AnalyticBinaryBarrierEngine{}.price(
                                 option, context(100.0, 0.04, 0.01, 0.3, expiry_date)),
                             kiyosi::RiskMeasure::price) == item.expected);
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
    const auto short_schedule = *kiyosi::make_cash_no_touch_down(
        valuation, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled,
        {valuation + std::chrono::days{30}, valuation + std::chrono::days{60}});
    const auto long_schedule = *kiyosi::make_cash_no_touch_down(
        valuation, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled,
        {valuation + std::chrono::days{179}, expiry_date});
    const auto short_value = risk_value(*kiyosi::AnalyticBinaryBarrierEngine{}.price(short_schedule, context()), kiyosi::RiskMeasure::price);
    const auto long_value = risk_value(*kiyosi::AnalyticBinaryBarrierEngine{}.price(long_schedule, context()), kiyosi::RiskMeasure::price);
    CHECK(std::abs(short_value - long_value) > 1e-4);

    const auto weekend = *kiyosi::make_cash_no_touch_down(
        valuation, expiry_date, 90.0, 10.0, kiyosi::ObservationMode::scheduled,
        {day(2025, 1, 11)});
    const auto market = *kiyosi::make_pricing_context(*kiyosi::make_bsm_parameters(0.04, 0.01, 0.3),
                                                      100.0, valuation,
                                                      kiyosi::weekdays_calendar());
    CHECK(kiyosi::AnalyticBinaryBarrierEngine{}.price(weekend, market).error().category ==
          kiyosi::ErrorCategory::invalid_schedule);
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
    const auto out = *kiyosi::make_barrier_option(terms);
    auto knock_in_terms = terms;
    knock_in_terms.barrier_type = kiyosi::BarrierType::down_and_in;
    const auto in = *kiyosi::make_barrier_option(knock_in_terms);
    const auto market = context();
    const double coarse = risk_value(*kiyosi::FiniteDifferenceBarrierEngine{80, 23}.price(out, market), kiyosi::RiskMeasure::price);
    const double fine = risk_value(*kiyosi::FiniteDifferenceBarrierEngine{240, 69}.price(out, market), kiyosi::RiskMeasure::price);
    const double analytic = risk_value(*kiyosi::AnalyticBarrierEngine{}.price(out, market), kiyosi::RiskMeasure::price);
    CHECK(std::abs(fine - analytic) < std::abs(coarse - analytic));
    CHECK(risk_value(*kiyosi::FiniteDifferenceBarrierEngine{240, 69}.price(in, market), kiyosi::RiskMeasure::price) > 0.0);

    auto weekend_terms = terms;
    weekend_terms.observation_dates = {day(2025, 1, 11)};
    const auto weekend = *kiyosi::make_barrier_option(weekend_terms);
    const auto weekdays_market = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.04, 0.01, 0.3), 100.0, valuation,
        kiyosi::weekdays_calendar());
    CHECK(kiyosi::AnalyticBarrierEngine{}.price(weekend, weekdays_market).error().category ==
          kiyosi::ErrorCategory::invalid_schedule);
    CHECK(kiyosi::FiniteDifferenceBarrierEngine{}.price(weekend, weekdays_market).error().category ==
          kiyosi::ErrorCategory::invalid_date);

    auto at_hit_terms = terms;
    at_hit_terms.rebate_timing = kiyosi::RebateTiming::at_hit;
    at_hit_terms.observation_dates = {valuation + std::chrono::days{37}, expiry_date};
    const auto at_hit = *kiyosi::make_barrier_option(at_hit_terms);
    CHECK(risk_value(*kiyosi::AnalyticBarrierEngine{}.price(at_hit, market), kiyosi::RiskMeasure::price) > 0.0);
}

TEST_CASE("Binomial and finite-difference prices converge toward analytic values")
{
    const auto option = *kiyosi::make_european_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto american_call = *kiyosi::make_american_option(
        kiyosi::OptionType::call, 100.0, valuation, expiry_date);
    const auto market = context(100.0, 0.04, 0.0, 0.3);
    const double reference = risk_value(*kiyosi::AnalyticVanillaEngine{}.price(option, market), kiyosi::RiskMeasure::price);

    std::array<double, 4> tree_errors{};
    for (std::size_t index = 0; index < tree_errors.size(); ++index) {
        const int steps = 64 << static_cast<int>(index);
        const auto result = kiyosi::CoxRossRubinsteinVanillaEngine{steps}.price(american_call, market);
        REQUIRE(result.has_value());
        tree_errors[index] = difference(risk_value(*result, kiyosi::RiskMeasure::price), reference);
    }
    CHECK(tree_errors.back() < tree_errors.front());
    CHECK(tree_errors[2] < tree_errors[0]);
    CHECK(tree_errors.front() / tree_errors.back() > 1.5);

    std::array<double, 3> finite_difference_errors{};
    for (std::size_t index = 0; index < finite_difference_errors.size(); ++index) {
        const int steps = 50 << static_cast<int>(index);
        const auto result = kiyosi::FiniteDifferenceVanillaEngine{steps, steps}.price(option, market);
        REQUIRE(result.has_value());
        finite_difference_errors[index] = difference(risk_value(*result, kiyosi::RiskMeasure::price), reference);
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

    for (const auto [rate, volatility] : {
             std::tuple{0.75, 0.125}, std::tuple{0.0, 0.25}, std::tuple{-3.0, 0.5}}) {
        const auto market = context(100.0, rate, 0.01, volatility);
        const auto stable = kiyosi::FiniteDifferenceSettings{4, 100, kiyosi::FiniteDifferenceScheme::explicit_euler};
        const auto boundary = kiyosi::FiniteDifferenceSettings{4, 2, kiyosi::FiniteDifferenceScheme::explicit_euler};
        const auto unstable = kiyosi::FiniteDifferenceSettings{4, 1, kiyosi::FiniteDifferenceScheme::explicit_euler};

        const auto vanilla_stable = kiyosi::FiniteDifferenceVanillaEngine{stable}.price(call, market);
        CHECK(vanilla_stable.has_value());
        const auto vanilla_boundary = kiyosi::FiniteDifferenceVanillaEngine{boundary}.price(call, market);
        CHECK((vanilla_boundary || vanilla_boundary.error().message != "explicit finite-difference grid is unstable"));
        CHECK_FALSE(kiyosi::FiniteDifferenceVanillaEngine{unstable}.price(call, market).has_value());
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

} // namespace
