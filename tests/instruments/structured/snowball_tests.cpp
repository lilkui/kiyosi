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
    mutable double maturity_coupon_rate{};
    mutable double minimal_coupon{};

    template <typename Option>
    kiyosi::Result<double> price(
        const Option& option, const kiyosi::PricingContext&) const
    {
        coupon_rates = option.knock_out_coupon_rates();
        maturity_coupon_rate = option.maturity_coupon_rate();
        if constexpr (requires { option.minimum_coupon_rate(); })
            minimal_coupon = option.minimum_coupon_rate();
        return coupon_rates.front() + maturity_coupon_rate;
    }
};

class CopyCountingNote {
public:
    CopyCountingNote(int& copies, kiyosi::Date effective_date, kiyosi::Date expiry_date)
        : copies_(&copies), observation_dates_{expiry_date}, effective_(effective_date), expiry_(expiry_date)
    {
    }

    CopyCountingNote(const CopyCountingNote& other)
        : copies_(other.copies_), knock_out_prices_(other.knock_out_prices_),
          observation_dates_(other.observation_dates_), effective_(other.effective_),
          expiry_(other.expiry_)
    {
        ++*copies_;
    }

    double initial_spot() const noexcept { return 100.0; }
    const std::vector<double>& knock_out_levels() const noexcept { return knock_out_prices_; }
    double upper_strike() const noexcept { return 100.0; }
    double lower_strike() const noexcept { return 60.0; }
    const std::vector<kiyosi::Date>& observation_dates() const noexcept { return observation_dates_; }
    double principal_ratio() const noexcept { return 1.0; }
    kiyosi::Date effective_date() const noexcept { return effective_; }
    kiyosi::Date expiry_date() const noexcept { return expiry_; }
    kiyosi::AutocallableBarrierState barrier_state() const noexcept
    {
        return kiyosi::AutocallableBarrierState::none;
    }

private:
    int* copies_;
    std::vector<double> knock_out_prices_{110.0};
    std::vector<kiyosi::Date> observation_dates_;
    kiyosi::Date effective_;
    kiyosi::Date expiry_;
};
} // namespace

TEST_CASE("Autocallable validation does not copy its input")
{
    int copies = 0;
    const CopyCountingNote note{copies, day(2025, 1, 1), day(2026, 1, 1)};

    REQUIRE(kiyosi::validate_autocallable_note(note));
    CHECK(copies == 0);
}

TEST_CASE("Autocallable factories preserve validation error categories")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto check = [](auto factory, auto terms) {
        terms.initial_spot = 0.0;
        CHECK(factory(terms).error().category == kiyosi::ErrorCategory::invalid_parameter);
        terms.initial_spot = 100.0;
        terms.observation_dates.clear();
        CHECK(factory(std::move(terms)).error().category == kiyosi::ErrorCategory::invalid_schedule);
    };

    check(kiyosi::make_phoenix_option,
          kiyosi::PhoenixTerms{.coupon_rate = 0.05,
                               .initial_spot = 100.0,
                               .knock_in_level = 60.0,
                               .knock_out_levels = {110.0},
                               .coupon_barrier_levels = {80.0},
                               .upper_strike = 100.0,
                               .lower_strike = 60.0,
                               .observation_dates = {expiry_date},
                               .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                               .effective_date = effective_date,
                               .expiry_date = expiry_date});
    check(kiyosi::make_binary_snowball_option,
          kiyosi::BinarySnowballTerms{.knock_out_coupon_rates = {0.05},
                                      .maturity_coupon_rate = 0.05,
                                      .initial_spot = 100.0,
                                      .knock_out_levels = {110.0},
                                      .upper_strike = 100.0,
                                      .lower_strike = 60.0,
                                      .observation_dates = {expiry_date},
                                      .effective_date = effective_date,
                                      .expiry_date = expiry_date});
    const kiyosi::BinarySnowballTerms binary_terms{.knock_out_coupon_rates = {0.05},
                                                   .maturity_coupon_rate = 0.05,
                                                   .initial_spot = 100.0,
                                                   .knock_out_levels = {110.0},
                                                   .upper_strike = 100.0,
                                                   .lower_strike = 60.0,
                                                   .observation_dates = {expiry_date},
                                                   .barrier_state = kiyosi::AutocallableBarrierState::knocked_in,
                                                   .effective_date = effective_date,
                                                   .expiry_date = expiry_date};
    const auto invalid_binary = kiyosi::make_binary_snowball_option(binary_terms);
    REQUIRE_FALSE(invalid_binary);
    CHECK(invalid_binary.error().category == kiyosi::ErrorCategory::invalid_parameter);
    auto knocked_out_binary = binary_terms;
    knocked_out_binary.barrier_state = kiyosi::AutocallableBarrierState::knocked_out;
    CHECK(kiyosi::make_binary_snowball_option(std::move(knocked_out_binary)).has_value());
    check(kiyosi::make_snowball_option,
          kiyosi::SnowballTerms{.knock_out_coupon_rates = {0.05},
                                .maturity_coupon_rate = 0.05,
                                .initial_spot = 100.0,
                                .knock_in_level = 60.0,
                                .knock_out_levels = {110.0},
                                .upper_strike = 100.0,
                                .lower_strike = 60.0,
                                .observation_dates = {expiry_date},
                                .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                .effective_date = effective_date,
                                .expiry_date = expiry_date});
    check(kiyosi::make_ternary_snowball_option,
          kiyosi::TernarySnowballTerms{.knock_out_coupon_rates = {0.05},
                                       .maturity_coupon_rate = 0.05,
                                       .minimum_coupon_rate = 0.01,
                                       .initial_spot = 100.0,
                                       .knock_in_level = 60.0,
                                       .knock_out_levels = {110.0},
                                       .upper_strike = 100.0,
                                       .lower_strike = 60.0,
                                       .observation_dates = {expiry_date},
                                       .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                       .effective_date = effective_date,
                                       .expiry_date = expiry_date});
}

TEST_CASE("Snowball factory rejects invalid schedules and accepts signed coupons")
{
    static_assert(!std::is_constructible_v<kiyosi::BinarySnowballOption, std::vector<double>, double, double,
                                           std::vector<double>, double, double, std::vector<kiyosi::Date>,
                                           kiyosi::AutocallableBarrierState, double, kiyosi::Date, kiyosi::Date>);

    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto note = kiyosi::make_snowball_option(kiyosi::SnowballTerms{
        .knock_out_coupon_rates = {-0.1},
        .maturity_coupon_rate = -0.05,
        .initial_spot = 100.0,
        .knock_in_level = 60.0,
        .knock_out_levels = {110.0},
        .upper_strike = 100.0,
        .lower_strike = 60.0,
        .observation_dates = {expiry_date},
        .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
        .barrier_state = kiyosi::AutocallableBarrierState::none,
        .principal_ratio = 1.0,
        .effective_date = effective_date,
        .expiry_date = expiry_date});
    REQUIRE(note);
    CHECK(kiyosi::make_snowball_option(kiyosi::SnowballTerms{
                                           .knock_out_coupon_rates = {0.1},
                                           .maturity_coupon_rate = 0.05,
                                           .initial_spot = 100.0,
                                           .knock_in_level = 60.0,
                                           .knock_out_levels = {110.0},
                                           .upper_strike = 100.0,
                                           .lower_strike = 60.0,
                                           .observation_dates = {expiry_date, effective_date},
                                           .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                           .barrier_state = kiyosi::AutocallableBarrierState::none,
                                           .principal_ratio = 1.0,
                                           .effective_date = effective_date,
                                           .expiry_date = expiry_date})
              .error()
              .category == kiyosi::ErrorCategory::invalid_schedule);
}

TEST_CASE("Named Snowball factories build DerivaSharp variants")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{day(2025, 4, 1), day(2025, 7, 1), expiry_date};

    const auto standard = kiyosi::make_standard_snowball({.coupon_rate = 0.1,
                                                          .initial_spot = 100.0,
                                                          .knock_in_level = 70.0,
                                                          .knock_out_level = 105.0,
                                                          .observation_dates = observation_dates,
                                                          .effective_date = effective_date,
                                                          .expiry_date = expiry_date});
    const auto step_down = kiyosi::make_step_down_snowball({.coupon_rate = 0.1,
                                                            .initial_spot = 100.0,
                                                            .knock_in_level = 70.0,
                                                            .initial_knock_out_level = 110.0,
                                                            .knock_out_level_decrement = 5.0,
                                                            .observation_dates = observation_dates,
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    const auto both_down = kiyosi::make_both_down_snowball({.initial_coupon_rate = 0.1,
                                                            .coupon_rate_decrement = 0.01,
                                                            .initial_spot = 100.0,
                                                            .knock_in_level = 70.0,
                                                            .initial_knock_out_level = 110.0,
                                                            .knock_out_level_decrement = 5.0,
                                                            .observation_dates = observation_dates,
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    const auto dual = kiyosi::make_dual_coupon_snowball({.knock_out_coupon_rate = 0.1,
                                                         .maturity_coupon_rate = 0.03,
                                                         .initial_spot = 100.0,
                                                         .knock_in_level = 70.0,
                                                         .knock_out_level = 105.0,
                                                         .observation_dates = observation_dates,
                                                         .effective_date = effective_date,
                                                         .expiry_date = expiry_date});
    const auto parachute = kiyosi::make_parachute_snowball({.coupon_rate = 0.1,
                                                            .initial_spot = 100.0,
                                                            .knock_in_level = 70.0,
                                                            .knock_out_level = 105.0,
                                                            .final_knock_out_level = 90.0,
                                                            .observation_dates = observation_dates,
                                                            .effective_date = effective_date,
                                                            .expiry_date = expiry_date});
    const auto otm = kiyosi::make_otm_snowball({.coupon_rate = 0.1,
                                                .initial_spot = 100.0,
                                                .knock_in_level = 70.0,
                                                .knock_out_level = 105.0,
                                                .upper_strike = 110.0,
                                                .observation_dates = observation_dates,
                                                .effective_date = effective_date,
                                                .expiry_date = expiry_date});
    const auto capped = kiyosi::make_loss_capped_snowball({.coupon_rate = 0.1,
                                                           .initial_spot = 100.0,
                                                           .knock_in_level = 70.0,
                                                           .knock_out_level = 105.0,
                                                           .lower_strike = 80.0,
                                                           .observation_dates = observation_dates,
                                                           .effective_date = effective_date,
                                                           .expiry_date = expiry_date});
    const auto european = kiyosi::make_european_snowball({.coupon_rate = 0.1,
                                                          .initial_spot = 100.0,
                                                          .knock_in_level = 70.0,
                                                          .knock_out_level = 105.0,
                                                          .observation_dates = observation_dates,
                                                          .effective_date = effective_date,
                                                          .expiry_date = expiry_date});

    REQUIRE(standard);
    REQUIRE(step_down);
    REQUIRE(both_down);
    REQUIRE(dual);
    REQUIRE(parachute);
    REQUIRE(otm);
    REQUIRE(capped);
    REQUIRE(european);
    CHECK(step_down->knock_out_levels()[1] == Catch::Approx(105.0));
    CHECK(both_down->knock_out_coupon_rates()[1] == Catch::Approx(0.09));
    CHECK(parachute->knock_out_levels().back() == Catch::Approx(90.0));
    CHECK(otm->upper_strike() == Catch::Approx(110.0));
    CHECK(capped->lower_strike() == Catch::Approx(80.0));
    CHECK(european->knock_in_observation_mode() == kiyosi::KnockInObservationMode::at_expiry);
    CHECK(dual->maturity_coupon_rate() == Catch::Approx(0.03));

    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, effective_date);
    const CouponSumEngine standard_engine;
    const CouponSumEngine both_down_engine;
    const CouponSumEngine dual_engine;
    const auto standard_coupon = kiyosi::implied_coupon(
        standard_engine, *standard, context, 0.24,
        kiyosi::CouponQuoteConvention::shift_maturity_coupon);
    const auto both_down_coupon = kiyosi::implied_coupon(
        both_down_engine, *both_down, context, 0.22,
        kiyosi::CouponQuoteConvention::shift_maturity_coupon);
    const auto dual_coupon = kiyosi::implied_coupon(
        dual_engine, *dual, context, 0.15,
        kiyosi::CouponQuoteConvention::preserve_maturity_coupon);
    REQUIRE(standard_coupon);
    REQUIRE(both_down_coupon);
    REQUIRE(dual_coupon);
    CHECK(*standard_coupon == Catch::Approx(0.12));
    CHECK(*both_down_coupon == Catch::Approx(0.12));
    CHECK(*dual_coupon == Catch::Approx(0.12));
    CHECK(standard_engine.maturity_coupon_rate == Catch::Approx(0.12));
    CHECK(both_down_engine.coupon_rates[1] == Catch::Approx(0.11));
    CHECK(both_down_engine.coupon_rates[2] == Catch::Approx(0.10));
    CHECK(both_down_engine.maturity_coupon_rate == Catch::Approx(0.10));
    CHECK(dual_engine.coupon_rates[1] == Catch::Approx(0.12));
    CHECK(dual_engine.maturity_coupon_rate == Catch::Approx(0.03));
}

TEST_CASE("Binary and ternary Snowballs imply knock-out coupons")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const std::vector<kiyosi::Date> observation_dates{day(2025, 7, 1), expiry_date};
    const auto binary = kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {0.10, 0.09},
                                                             .maturity_coupon_rate = 0.03,
                                                             .initial_spot = 100.0,
                                                             .knock_out_levels = {105.0, 100.0},
                                                             .upper_strike = 100.0,
                                                             .lower_strike = 0.0,
                                                             .observation_dates = observation_dates,
                                                             .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                             .principal_ratio = 1.0,
                                                             .effective_date = effective_date,
                                                             .expiry_date = expiry_date});
    const auto ternary = kiyosi::make_ternary_snowball_option({.knock_out_coupon_rates = {0.10, 0.09},
                                                               .maturity_coupon_rate = 0.03,
                                                               .minimum_coupon_rate = 0.01,
                                                               .initial_spot = 100.0,
                                                               .knock_in_level = 70.0,
                                                               .knock_out_levels = {105.0, 100.0},
                                                               .upper_strike = 100.0,
                                                               .lower_strike = 0.0,
                                                               .observation_dates = observation_dates,
                                                               .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day,
                                                               .barrier_state = kiyosi::AutocallableBarrierState::none,
                                                               .principal_ratio = 1.0,
                                                               .effective_date = effective_date,
                                                               .expiry_date = expiry_date});
    REQUIRE(binary);
    REQUIRE(ternary);

    const auto parameters = *kiyosi::make_bsm_parameters(0.04, 0.01, 0.2);
    const auto context = *kiyosi::make_pricing_context(parameters, 100.0, effective_date);
    const CouponSumEngine binary_engine;
    const CouponSumEngine ternary_engine;
    const auto implied_binary = kiyosi::implied_coupon(
        binary_engine, *binary, context, 0.15,
        kiyosi::CouponQuoteConvention::preserve_maturity_coupon);
    const auto implied_ternary = kiyosi::implied_coupon(
        ternary_engine, *ternary, context, 0.15,
        kiyosi::CouponQuoteConvention::preserve_maturity_coupon);
    REQUIRE(implied_binary);
    REQUIRE(implied_ternary);
    CHECK(*implied_binary == Catch::Approx(0.12));
    CHECK(*implied_ternary == Catch::Approx(0.12));
    CHECK(binary_engine.coupon_rates[1] == Catch::Approx(0.11));
    CHECK(binary_engine.maturity_coupon_rate == Catch::Approx(0.03));
    CHECK(ternary_engine.coupon_rates[1] == Catch::Approx(0.11));
    CHECK(ternary_engine.maturity_coupon_rate == Catch::Approx(0.03));
    CHECK(ternary_engine.minimal_coupon == Catch::Approx(0.01));
}

TEST_CASE("Negative Binary Snowball coupon can be implied")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto option = kiyosi::make_binary_snowball_option({.knock_out_coupon_rates = {-0.1},
        .maturity_coupon_rate = -0.1, .initial_spot = 100.0, .knock_out_levels = {1.0},
        .upper_strike = 100.0, .lower_strike = 60.0, .observation_dates = {expiry_date},
        .effective_date = effective_date, .expiry_date = expiry_date});
    REQUIRE(option);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, effective_date);
    const kiyosi::MonteCarloBinarySnowballEngine engine{{32, 73}};
    const auto price = engine.price(*option, context);
    REQUIRE(price);
    const auto implied = kiyosi::implied_coupon(engine, *option, context, *price,
        kiyosi::CouponQuoteConvention::preserve_maturity_coupon, {-0.2, 0.2});
    REQUIRE(implied);
    CHECK(*implied == Catch::Approx(-0.1).margin(1e-7));
}
