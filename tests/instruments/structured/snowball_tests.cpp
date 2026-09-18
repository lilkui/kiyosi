#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>

#include <chrono>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;

struct CouponSumEngine {
    mutable std::vector<double> coupon_rates;
    mutable double maturity_coupon{};
    mutable double minimal_coupon{};

    template <typename Option>
    kiyosi::result<kiyosi::PricingResult> price(
        const Option& option, const kiyosi::PricingContext&) const
    {
        coupon_rates = option.knock_out_coupon_rates();
        maturity_coupon = option.maturity_coupon_rate();
        if constexpr (requires { option.minimal_coupon_rate(); })
            minimal_coupon = option.minimal_coupon_rate();
        return kiyosi::make_pricing_result(
            {{kiyosi::risk_measure::price, coupon_rates.front() + maturity_coupon}});
    }
};

class CopyCountingNote {
public:
    CopyCountingNote(int& copies, kiyosi::date effective, kiyosi::date expiry)
        : copies_(&copies), observation_dates_{expiry}, effective_(effective), expiry_(expiry)
    {}

    CopyCountingNote(const CopyCountingNote& other)
        : copies_(other.copies_), knock_out_prices_(other.knock_out_prices_),
          observation_dates_(other.observation_dates_), effective_(other.effective_),
          expiry_(other.expiry_)
    {
        ++*copies_;
    }

    double initial_price() const noexcept { return 100.0; }
    const std::vector<double>& knock_out_prices() const noexcept { return knock_out_prices_; }
    double upper_strike() const noexcept { return 100.0; }
    double lower_strike() const noexcept { return 60.0; }
    const std::vector<kiyosi::date>& observation_dates() const noexcept { return observation_dates_; }
    double principal_ratio() const noexcept { return 1.0; }
    kiyosi::date effective() const noexcept { return effective_; }
    kiyosi::date expiry() const noexcept { return expiry_; }
    kiyosi::barrier_touch_status touch_status() const noexcept
    {
        return kiyosi::barrier_touch_status::none;
    }

private:
    int* copies_;
    std::vector<double> knock_out_prices_{110.0};
    std::vector<kiyosi::date> observation_dates_;
    kiyosi::date effective_;
    kiyosi::date expiry_;
};
} // namespace

TEST_CASE("Autocallable validation does not copy its input")
{
    int copies = 0;
    const CopyCountingNote note{copies, day(2025, 1, 1), day(2026, 1, 1)};

    REQUIRE(kiyosi::validate_note(note));
    CHECK(copies == 0);
}

TEST_CASE("Autocallable factories preserve validation error categories")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto check = [](auto factory, auto terms) {
        terms.initial_price = 0.0;
        CHECK(factory(terms).error().category == kiyosi::error_category::invalid_parameter);
        terms.initial_price = 100.0;
        terms.observation_dates.clear();
        CHECK(factory(std::move(terms)).error().category == kiyosi::error_category::invalid_schedule);
    };

    check(kiyosi::make_phoenix_option,
          kiyosi::PhoenixTerms{.coupon_rate = 0.05,
                               .initial_price = 100.0,
                               .knock_in_price = 60.0,
                               .knock_out_prices = {110.0},
                               .coupon_barriers = {80.0},
                               .upper_strike = 100.0,
                               .lower_strike = 60.0,
                               .observation_dates = {expiry},
                               .frequency = kiyosi::observation_frequency::daily,
                               .effective = effective,
                               .expiry = expiry});
    check(kiyosi::make_binary_snowball_option,
          kiyosi::BinarySnowballTerms{.knock_out_coupon_rates = {0.05},
                                      .maturity_coupon_rate = 0.05,
                                      .initial_price = 100.0,
                                      .knock_out_prices = {110.0},
                                      .upper_strike = 100.0,
                                      .lower_strike = 60.0,
                                      .observation_dates = {expiry},
                                      .effective = effective,
                                      .expiry = expiry});
    check(kiyosi::make_snowball_option,
          kiyosi::SnowballTerms{.knock_out_coupon_rates = {0.05},
                                .maturity_coupon_rate = 0.05,
                                .initial_price = 100.0,
                                .knock_in_price = 60.0,
                                .knock_out_prices = {110.0},
                                .upper_strike = 100.0,
                                .lower_strike = 60.0,
                                .observation_dates = {expiry},
                                .frequency = kiyosi::observation_frequency::daily,
                                .effective = effective,
                                .expiry = expiry});
    check(kiyosi::make_ternary_snowball_option,
          kiyosi::TernarySnowballTerms{.knock_out_coupon_rates = {0.05},
                                       .maturity_coupon_rate = 0.05,
                                       .minimal_coupon_rate = 0.01,
                                       .initial_price = 100.0,
                                       .knock_in_price = 60.0,
                                       .knock_out_prices = {110.0},
                                       .upper_strike = 100.0,
                                       .lower_strike = 60.0,
                                       .observation_dates = {expiry},
                                       .frequency = kiyosi::observation_frequency::daily,
                                       .effective = effective,
                                       .expiry = expiry});
}

TEST_CASE("Snowball factory rejects invalid schedules and accepts signed coupons")
{
    static_assert(!std::is_constructible_v<kiyosi::BinarySnowballOption, std::vector<double>, double, double,
                                           std::vector<double>, double, double, std::vector<kiyosi::date>,
                                           kiyosi::barrier_touch_status, double, kiyosi::date, kiyosi::date>);

    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto note = kiyosi::make_snowball_option(kiyosi::SnowballTerms{
        .knock_out_coupon_rates = {-0.1},
        .maturity_coupon_rate = -0.05,
        .initial_price = 100.0,
        .knock_in_price = 60.0,
        .knock_out_prices = {110.0},
        .upper_strike = 100.0,
        .lower_strike = 60.0,
        .observation_dates = {expiry},
        .frequency = kiyosi::observation_frequency::daily,
        .touch_status = kiyosi::barrier_touch_status::none,
        .principal_ratio = 1.0,
        .effective = effective,
        .expiry = expiry});
    REQUIRE(note);
    CHECK(kiyosi::make_snowball_option(kiyosi::SnowballTerms{
                                           .knock_out_coupon_rates = {0.1},
                                           .maturity_coupon_rate = 0.05,
                                           .initial_price = 100.0,
                                           .knock_in_price = 60.0,
                                           .knock_out_prices = {110.0},
                                           .upper_strike = 100.0,
                                           .lower_strike = 60.0,
                                           .observation_dates = {expiry, effective},
                                           .frequency = kiyosi::observation_frequency::daily,
                                           .touch_status = kiyosi::barrier_touch_status::none,
                                           .principal_ratio = 1.0,
                                           .effective = effective,
                                           .expiry = expiry})
              .error()
              .category == kiyosi::error_category::invalid_schedule);
}

TEST_CASE("Named Snowball factories build DerivaSharp variants")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 4, 1), day(2025, 7, 1), expiry};

    const auto standard = kiyosi::make_standard_snowball(0.1, 100.0, 70.0, 105.0, observation_dates, effective, expiry);
    const auto step_down = kiyosi::make_step_down_snowball(0.1, 100.0, 70.0, 110.0, 5.0, observation_dates, effective, expiry);
    const auto both_down = kiyosi::make_both_down_snowball(0.1, 0.01, 100.0, 70.0, 110.0, 5.0, observation_dates, effective, expiry);
    const auto dual = kiyosi::make_dual_coupon_snowball(0.1, 0.03, 100.0, 70.0, 105.0, observation_dates, effective, expiry);
    const auto parachute = kiyosi::make_parachute_snowball(0.1, 100.0, 70.0, 105.0, 90.0, observation_dates, effective, expiry);
    const auto otm = kiyosi::make_otm_snowball(0.1, 100.0, 70.0, 105.0, 110.0, observation_dates, effective, expiry);
    const auto capped = kiyosi::make_loss_capped_snowball(0.1, 100.0, 70.0, 105.0, 80.0, observation_dates, effective, expiry);
    const auto european = kiyosi::make_european_snowball(0.1, 100.0, 70.0, 105.0, observation_dates, effective, expiry);

    REQUIRE(standard);
    REQUIRE(step_down);
    REQUIRE(both_down);
    REQUIRE(dual);
    REQUIRE(parachute);
    REQUIRE(otm);
    REQUIRE(capped);
    REQUIRE(european);
    CHECK(step_down->knock_out_prices()[1] == Catch::Approx(105.0));
    CHECK(both_down->knock_out_coupon_rates()[1] == Catch::Approx(0.09));
    CHECK(parachute->knock_out_prices().back() == Catch::Approx(90.0));
    CHECK(otm->upper_strike() == Catch::Approx(110.0));
    CHECK(capped->lower_strike() == Catch::Approx(80.0));
    CHECK(european->knock_in_frequency() == kiyosi::observation_frequency::at_expiry);
    CHECK(dual->maturity_coupon_rate() == Catch::Approx(0.03));

    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, effective);
    const CouponSumEngine standard_engine;
    const CouponSumEngine both_down_engine;
    const CouponSumEngine dual_engine;
    const auto standard_coupon = kiyosi::implied_coupon(
        standard_engine, *standard, context, 0.24,
        kiyosi::coupon_quote_convention::linked_maturity);
    const auto both_down_coupon = kiyosi::implied_coupon(
        both_down_engine, *both_down, context, 0.22,
        kiyosi::coupon_quote_convention::linked_maturity);
    const auto dual_coupon = kiyosi::implied_coupon(
        dual_engine, *dual, context, 0.15,
        kiyosi::coupon_quote_convention::fixed_maturity);
    REQUIRE(standard_coupon);
    REQUIRE(both_down_coupon);
    REQUIRE(dual_coupon);
    CHECK(*standard_coupon == Catch::Approx(0.12));
    CHECK(*both_down_coupon == Catch::Approx(0.12));
    CHECK(*dual_coupon == Catch::Approx(0.12));
    CHECK(standard_engine.maturity_coupon == Catch::Approx(0.12));
    CHECK(both_down_engine.coupon_rates[1] == Catch::Approx(0.11));
    CHECK(both_down_engine.coupon_rates[2] == Catch::Approx(0.10));
    CHECK(both_down_engine.maturity_coupon == Catch::Approx(0.10));
    CHECK(dual_engine.coupon_rates[1] == Catch::Approx(0.12));
    CHECK(dual_engine.maturity_coupon == Catch::Approx(0.03));
}

TEST_CASE("Binary and ternary Snowballs imply knock-out coupons")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const std::vector<kiyosi::date> observation_dates{day(2025, 7, 1), expiry};
    const auto binary = kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.10, 0.09},
                                                             .maturity_coupon_rate = 0.03,
                                                             .initial_price = 100.0,
                                                             .knock_out_prices = {105.0, 100.0},
                                                             .upper_strike = 100.0,
                                                             .lower_strike = 0.0,
                                                             .observation_dates = observation_dates,
                                                             .touch_status = kiyosi::barrier_touch_status::none,
                                                             .principal_ratio = 1.0,
                                                             .effective = effective,
                                                             .expiry = expiry});
    const auto ternary = kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.10, 0.09},
                                                               .maturity_coupon_rate = 0.03,
                                                               .minimal_coupon_rate = 0.01,
                                                               .initial_price = 100.0,
                                                               .knock_in_price = 70.0,
                                                               .knock_out_prices = {105.0, 100.0},
                                                               .upper_strike = 100.0,
                                                               .lower_strike = 0.0,
                                                               .observation_dates = observation_dates,
                                                               .frequency = kiyosi::observation_frequency::daily,
                                                               .touch_status = kiyosi::barrier_touch_status::none,
                                                               .principal_ratio = 1.0,
                                                               .effective = effective,
                                                               .expiry = expiry});
    REQUIRE(binary);
    REQUIRE(ternary);

    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, effective);
    const CouponSumEngine binary_engine;
    const CouponSumEngine ternary_engine;
    const auto implied_binary = kiyosi::implied_coupon(
        binary_engine, *binary, context, 0.15,
        kiyosi::coupon_quote_convention::fixed_maturity);
    const auto implied_ternary = kiyosi::implied_coupon(
        ternary_engine, *ternary, context, 0.15,
        kiyosi::coupon_quote_convention::fixed_maturity);
    REQUIRE(implied_binary);
    REQUIRE(implied_ternary);
    CHECK(*implied_binary == Catch::Approx(0.12));
    CHECK(*implied_ternary == Catch::Approx(0.12));
    CHECK(binary_engine.coupon_rates[1] == Catch::Approx(0.11));
    CHECK(binary_engine.maturity_coupon == Catch::Approx(0.03));
    CHECK(ternary_engine.coupon_rates[1] == Catch::Approx(0.11));
    CHECK(ternary_engine.maturity_coupon == Catch::Approx(0.03));
    CHECK(ternary_engine.minimal_coupon == Catch::Approx(0.01));
}
