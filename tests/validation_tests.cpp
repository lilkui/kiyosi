#include <catch2/catch_test_macros.hpp>

#include <array>
#include <chrono>
#include <cmath>
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

void check_close(double actual, double expected, double absolute = 1e-7, double relative = 1e-5)
{
    CHECK(difference(actual, expected) <= absolute + relative * std::abs(expected));
}

double value(kiyosi::option_type type, double spot, double rate, double dividend,
             double volatility, kiyosi::date value_date, kiyosi::date option_expiry, double strike = 100.0)
{
    return analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike).value;
}

double delta(kiyosi::option_type type, double spot, double rate, double dividend,
             double volatility, kiyosi::date value_date, kiyosi::date option_expiry, double strike = 100.0)
{
    return analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike).delta;
}

double gamma(kiyosi::option_type type, double spot, double rate, double dividend,
             double volatility, kiyosi::date value_date, kiyosi::date option_expiry, double strike = 100.0)
{
    return analytic(type, spot, rate, dividend, volatility, value_date, option_expiry, strike).gamma;
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
    check_close(result.delta, delta_fd, 2e-7, 2e-4);
    check_close(result.gamma, gamma_fd, 2e-7, 2e-4);
    check_close(result.speed, speed_fd, 2e-6, 2e-3);

    const double theta_fd = (value(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry) -
                             value(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry)) /
                            2.0;
    const double charm_fd = (delta(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry) -
                             delta(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry)) /
                            2.0;
    const double color_fd = (gamma(type, 100.0, 0.04, 0.01, 0.3, next_day, expiry) -
                             gamma(type, 100.0, 0.04, 0.01, 0.3, previous_day, expiry)) /
                            2.0;
    check_close(result.theta, theta_fd, 2e-6, 2e-3);
    check_close(result.charm, charm_fd, 2e-6, 2e-3);
    check_close(result.color, color_fd, 2e-6, 2e-3);

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
    check_close(result.vega, vega_fd, 2e-6, 2e-4);
    check_close(result.vanna, vanna_fd, 2e-6, 2e-3);
    check_close(result.zomma, zomma_fd, 2e-6, 2e-3);
    check_close(result.rho, rho_fd, 2e-6, 2e-4);
    }
}

TEST_CASE("Analytic pricing satisfies no-arbitrage identities")
{
    const auto call = analytic(kiyosi::option_type::call);
    const auto put = analytic(kiyosi::option_type::put);
    const double time = 1.0;
    check_close(call.value - put.value, 100.0 * std::exp(-0.01 * time) - 100.0 * std::exp(-0.04 * time), 1e-10, 1e-10);
    check_close(call.delta - put.delta, std::exp(-0.01 * time), 1e-10, 1e-10);
    check_close(call.gamma, put.gamma, 1e-10, 1e-10);
    check_close(call.vega, put.vega, 1e-10, 1e-10);

    const kiyosi::AnalyticDigitalEngine digital;
    for (const auto type : {kiyosi::option_type::call, kiyosi::option_type::put}) {
        const auto cash = *kiyosi::make_cash_or_nothing_option(type, 100.0, 100.0, expiry);
        const auto asset = *kiyosi::make_asset_or_nothing_option(type, 100.0, expiry);
        const auto vanilla = analytic(type);
        const auto cash_value = digital.price(cash, context())->value;
        const auto asset_value = digital.price(asset, context())->value;
        check_close(type == kiyosi::option_type::call ? asset_value - cash_value : cash_value - asset_value,
                    vanilla.value, 2e-10, 2e-10);
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
                                 : kind == kiyosi::barrier_type::up_and_out ? kiyosi::barrier_type::up_and_in
                                 : kind == kiyosi::barrier_type::down_and_in ? kiyosi::barrier_type::down_and_out
                                                                          : kiyosi::barrier_type::down_and_in;
        const auto paired = *kiyosi::make_barrier_option(
            kiyosi::option_type::call, 100.0, expiry,
            kind == kiyosi::barrier_type::up_and_in || kind == kiyosi::barrier_type::up_and_out ? 130.0 : 75.0,
            paired_kind);
        check_close(barriers.price(option, context())->value + barriers.price(paired, context())->value,
                    analytic(kiyosi::option_type::call).value, 2e-5, 2e-5);
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
                          item.volatility, value_date, option_expiry, item.strike), item.call, 2e-8, 2e-8);
        check_close(value(kiyosi::option_type::put, item.spot, item.rate, item.dividend,
                          item.volatility, value_date, option_expiry, item.strike), item.put, 2e-8, 2e-8);
    }
}

TEST_CASE("Binomial and finite-difference prices converge toward analytic values")
{
    const auto option = *kiyosi::make_european_call(100.0, expiry);
    const auto american_call = *kiyosi::make_american_call(100.0, expiry);
    const auto market = context(100.0, 0.04, 0.0, 0.3);
    const double reference = kiyosi::AnalyticEuropeanEngine{}.price(option, market)->value;

    std::array<double, 4> tree_errors{};
    for (std::size_t index = 0; index < tree_errors.size(); ++index) {
        const int steps = 64 << static_cast<int>(index);
        const auto result = kiyosi::BinomialAmericanEngine{steps}.price(american_call, market);
        REQUIRE(result.has_value());
        tree_errors[index] = difference(result->value, reference);
    }
    CHECK(tree_errors.back() < tree_errors.front());
    CHECK(tree_errors[2] < tree_errors[0]);
    CHECK(tree_errors.front() / tree_errors.back() > 1.5);

    std::array<double, 3> finite_difference_errors{};
    for (std::size_t index = 0; index < finite_difference_errors.size(); ++index) {
        const int steps = 50 << static_cast<int>(index);
        const auto result = kiyosi::FiniteDifferenceEuropeanEngine{steps, steps}.price(option, market);
        REQUIRE(result.has_value());
        finite_difference_errors[index] = difference(result->value, reference);
    }
    CHECK(finite_difference_errors.back() < finite_difference_errors.front());
    CHECK(finite_difference_errors[2] < finite_difference_errors[1]);
    CHECK(finite_difference_errors.front() / finite_difference_errors.back() > 1.5);
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
              .error().category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_asset_or_nothing_option(kiyosi::option_type::call, 0.0, expiry)
              .error().category == kiyosi::error_category::invalid_strike);
    CHECK(kiyosi::make_cash_or_nothing_option(kiyosi::option_type::call, 100.0, 0.0, expiry)
              .error().category == kiyosi::error_category::invalid_parameter);

    CHECK(kiyosi::make_bermudan_option(terms, kiyosi::VanillaPayoff{}, {}).error().category ==
          kiyosi::error_category::invalid_schedule);
    CHECK(kiyosi::make_bermudan_option(
              terms, kiyosi::VanillaPayoff{},
              std::vector{valuation + std::chrono::days{30}, valuation + std::chrono::days{30}})
              .error().category == kiyosi::error_category::invalid_schedule);
    CHECK(kiyosi::make_bermudan_option(
              terms, kiyosi::VanillaPayoff{}, std::vector{day(2025, 1, 11)}, kiyosi::exchange_calendar())
              .error().category == kiyosi::error_category::invalid_schedule);

    const auto later_terms = *kiyosi::make_option_terms(
        kiyosi::option_type::call, 100.0, expiry + std::chrono::days{30});
    const auto later_exercise = *kiyosi::make_bermudan_exercise(
        std::vector{expiry + std::chrono::days{1}}, later_terms.expiry());
    CHECK(kiyosi::make_exercise_based_option(terms, kiyosi::VanillaPayoff{}, later_exercise)
              .error().category == kiyosi::error_category::invalid_schedule);
}

} // namespace
