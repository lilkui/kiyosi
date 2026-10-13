#include <catch2/catch_test_macros.hpp>
#include <catch2/catch_approx.hpp>

#include <chrono>
#include <limits>
#include <optional>
#include <type_traits>
#include <utility>
#include <vector>

#include <kiyosi/kiyosi.hpp>
#include <kiyosi/pricing/detail/autocallable_traits.hpp>

#include "support/common.hpp"

namespace {
using kiyosi::test::day;

TEST_CASE("Autocallable factories accept associated observation records", "[observation-record-input]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), first = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto check = [&](auto factory, auto terms) {
        terms.observations = {{first, 115.0}, {end, 110.0}};
        if constexpr (requires { terms.coupon_rate; }) {
            terms.observations[0].coupon_barrier_level = 85.0;
            terms.observations[1].coupon_barrier_level = 80.0;
        } else {
            terms.observations[0].coupon_rate = 0.08;
            terms.observations[1].coupon_rate = 0.12;
        }
        const auto record_note = factory(terms);
        REQUIRE(record_note);
        const auto original = *record_note; // NOLINT(performance-unnecessary-copy-initialization): snapshot detects mutation.
        CHECK(record_note->observations_view()[0] == terms.observations[0]);
        CHECK(record_note->observations_view()[1] == terms.observations[1]);
        std::ranges::reverse(terms.observations);
        const auto unordered = factory(terms);
        REQUIRE_FALSE(unordered);
        CHECK(unordered.error().category == ErrorCategory::invalid_schedule);
        std::ranges::sort(terms.observations, {}, &AutocallableObservation::date);
        CHECK(*factory(terms) == *record_note);
        CHECK(terms.observations.front().knock_out_level == 115.0);
        terms.observations.front().knock_out_level = -1.0;
        const auto invalid_level = factory(terms);
        REQUIRE_FALSE(invalid_level);
        CHECK(invalid_level.error().category == ErrorCategory::invalid_parameter);
        CHECK(*record_note == original);
        terms.observations.front().knock_out_level = 115.0;
        if constexpr (requires { terms.coupon_rate; }) terms.observations.front().coupon_rate = 0.1;
        else terms.observations.front().coupon_barrier_level = 1.0;
        const auto unused_field = factory(terms);
        REQUIRE_FALSE(unused_field);
        CHECK(unused_field.error().category == ErrorCategory::invalid_parameter);
    };
    check(make_binary_snowball_option, BinarySnowballTerms{.maturity_coupon_rate = 0.05, .effective_date = start, .expiry_date = end});
    check(make_ternary_snowball_option, TernarySnowballTerms{.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.01, .knock_in_level = 75.0, .effective_date = start, .expiry_date = end});
    check(make_snowball_option, SnowballTerms{.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .effective_date = start, .expiry_date = end});
    check(make_phoenix_option, PhoenixTerms{.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 60.0, .effective_date = start, .expiry_date = end});
}

TEST_CASE("Observation sequence adapter preserves associations and rejects mismatched lengths", "[observation-record-input]")
{
    using namespace kiyosi;
    const auto first = day(2025, 7, 1), end = day(2026, 1, 1);
    const std::vector<Date> dates{first, end};
    const std::vector<double> levels{115.0, 110.0};
    const auto coupons = make_autocallable_observations(dates, levels, std::vector{0.08, 0.12});
    REQUIRE(coupons);
    CHECK(*coupons == std::vector<AutocallableObservation>{{first, 115.0, 0.08}, {end, 110.0, 0.12}});
    const auto barriers = make_autocallable_observations(dates, levels, std::nullopt, std::vector{85.0, 80.0});
    REQUIRE(barriers);
    CHECK(*barriers == std::vector<AutocallableObservation>{{first, 115.0, 0.0, 85.0}, {end, 110.0, 0.0, 80.0}});
    for (const auto& invalid : {
             make_autocallable_observations(dates, {115.0}),
             make_autocallable_observations(dates, levels, std::vector<double>{}),
             make_autocallable_observations(dates, levels, std::vector{0.08}),
             make_autocallable_observations(dates, levels, std::nullopt, std::vector<double>{}),
             make_autocallable_observations(dates, levels, std::nullopt, std::vector{85.0})}) {
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_schedule);
    }
}

TEST_CASE("Coupon replacement preserves owning notes and validates changed terms", "[coupon-replacement]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), first = day(2025, 7, 1), end = day(2026, 1, 1);
    const auto check = [](auto factory, const auto& terms) {
        const auto note = factory(terms);
        REQUIRE(note);
        const auto original = *note; // NOLINT(performance-unnecessary-copy-initialization): snapshot detects mutation.
        for (const auto convention : {CouponQuoteConvention::shift_maturity_coupon, CouponQuoteConvention::preserve_maturity_coupon}) {
            auto expected_terms = terms;
            expected_terms.observations[0].coupon_rate = 0.375;
            expected_terms.observations[1].coupon_rate = 0.5;
            expected_terms.maturity_coupon_rate = convention == CouponQuoteConvention::shift_maturity_coupon ? 0.3125 : 0.0625;
            const auto expected = factory(expected_terms);
            const auto replaced = detail::replace_coupon(*note, 0.375, convention);
            REQUIRE(expected);
            REQUIRE(replaced);
            CHECK(*replaced == *expected);
            CHECK(replaced->observations_view().data() != note->observations_view().data());
            CHECK(*note == original);
        }
        for (const double invalid : {std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity()}) {
            const auto rejected = detail::replace_coupon(*note, invalid, CouponQuoteConvention::preserve_maturity_coupon);
            REQUIRE_FALSE(rejected);
            CHECK(rejected.error().category == ErrorCategory::invalid_parameter);
            CHECK(*note == original);
        }
        const auto invalid_convention = detail::replace_coupon(*note, 0.375, static_cast<CouponQuoteConvention>(255));
        REQUIRE_FALSE(invalid_convention);
        CHECK(invalid_convention.error().category == ErrorCategory::invalid_parameter);
        auto overflow_terms = terms;
        overflow_terms.observations[0].coupon_rate = -std::numeric_limits<double>::max();
        overflow_terms.observations[1].coupon_rate = std::numeric_limits<double>::max();
        const auto overflow_note = factory(overflow_terms);
        REQUIRE(overflow_note);
        const auto overflow = detail::replace_coupon(*overflow_note, 0.0, CouponQuoteConvention::preserve_maturity_coupon);
        REQUIRE_FALSE(overflow);
        CHECK(overflow.error().category == ErrorCategory::invalid_parameter);
    };
    check(make_snowball_option, SnowballTerms{.maturity_coupon_rate = 0.0625, .initial_spot = 105.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 55.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::knocked_in, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.125}, {end, 110.0, 0.25}}});
    check(make_binary_snowball_option, BinarySnowballTerms{.maturity_coupon_rate = 0.0625, .barrier_state = AutocallableBarrierState::none, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.125}, {end, 110.0, 0.25}}});
    const TernarySnowballTerms ternary_terms{.maturity_coupon_rate = 0.0625, .minimum_coupon_rate = 0.015625, .knock_in_level = 75.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::knocked_in, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.125}, {end, 110.0, 0.25}}};
    check(make_ternary_snowball_option, ternary_terms);
    const auto ternary = make_ternary_snowball_option(ternary_terms);
    REQUIRE(ternary);
    const auto below_floor = detail::replace_coupon(*ternary, 0.0, CouponQuoteConvention::shift_maturity_coupon);
    REQUIRE_FALSE(below_floor);
    CHECK(below_floor.error().category == ErrorCategory::invalid_parameter);
    CHECK(*ternary == *make_ternary_snowball_option(ternary_terms));

    PhoenixTerms phoenix_terms{.coupon_rate = 0.125, .initial_spot = 105.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 55.0, .knock_in_observation_mode = KnockInObservationMode::at_expiry, .barrier_state = AutocallableBarrierState::none, .principal_ratio = 1.5, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.0, 85.0}, {end, 110.0, 0.0, 80.0}}};
    const auto phoenix = make_phoenix_option(phoenix_terms);
    REQUIRE(phoenix);
    const auto original = *phoenix; // NOLINT(performance-unnecessary-copy-initialization): snapshot detects mutation.
    phoenix_terms.coupon_rate = 0.375;
    const auto replaced = detail::replace_coupon(*phoenix, 0.375);
    REQUIRE(replaced);
    CHECK(*replaced == *make_phoenix_option(phoenix_terms));
    CHECK(replaced->observations_view().data() != phoenix->observations_view().data());
    for (const double invalid : {std::numeric_limits<double>::quiet_NaN(), std::numeric_limits<double>::infinity()}) {
        const auto rejected = detail::replace_coupon(*phoenix, invalid);
        REQUIRE_FALSE(rejected);
        CHECK(rejected.error().category == ErrorCategory::invalid_parameter);
    }
    CHECK(*phoenix == original);
}

TEST_CASE("Autocallable records retain each date level and coupon association", "[autocallable-records]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), first = day(2025, 1, 11), end = day(2025, 1, 31);
    const auto snowball = make_binary_snowball_option(
        {.maturity_coupon_rate = 0.05, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{first, 110.0, 0.365}, {end, 105.0, 0.73}}});
    REQUIRE(snowball);
    const auto observations = snowball->observations_view();
    REQUIRE(observations.size() == 2);
    CHECK(observations[0] == AutocallableObservation{first, 110.0, 0.365, 0.0});
    CHECK(observations[1] == AutocallableObservation{end, 105.0, 0.73, 0.0});
    CHECK(detail::autocallable_event(*snowball, 0).coupon == Catch::Approx(0.01));
    CHECK(detail::autocallable_event(*snowball, 1).coupon == Catch::Approx(0.06));

    const auto phoenix = make_phoenix_option(
        {.coupon_rate = 0.365, .initial_spot = 100.0, .knock_in_level = 70.0, .upper_strike = 100.0, .lower_strike = 60.0, .barrier_state = AutocallableBarrierState::none, .effective_date = start, .expiry_date = end, .observations = {{first, 110.0, 0.0, 108.0}, {end, 105.0, 0.0, 102.0}}});
    REQUIRE(phoenix);
    CHECK(phoenix->observations_view()[0] == AutocallableObservation{first, 110.0, 0.0, 108.0});
    CHECK(phoenix->observations_view()[1] == AutocallableObservation{end, 105.0, 0.0, 102.0});
    CHECK(detail::autocallable_event(*phoenix, 1).coupon == Catch::Approx(0.02));
    const auto context = *make_pricing_context(*make_bsm_parameters(0.04, 0.01, 0.2), 106.0, end, all_days_calendar());
    for (const auto& price : {FiniteDifferenceAutocallableEngine{}.price(*snowball, context),
                              MonteCarloAutocallableEngine{8, 7}.price(*snowball, context)}) {
        REQUIRE(price);
        CHECK(*price == Catch::Approx(1.06));
    }
    for (const auto& price : {FiniteDifferenceAutocallableEngine{}.price(*phoenix, context),
                              MonteCarloAutocallableEngine{8, 7}.price(*phoenix, context)}) {
        REQUIRE(price);
        CHECK(*price == Catch::Approx(1.02));
    }

    BinarySnowballTerms mismatched{.maturity_coupon_rate = 0.05, .principal_ratio = -1.0, .effective_date = start, .expiry_date = end, .observations = {}};
    const auto invalid_principal = make_binary_snowball_option(mismatched);
    REQUIRE_FALSE(invalid_principal);
    CHECK(invalid_principal.error().category == ErrorCategory::invalid_parameter);
    mismatched.principal_ratio = 1.0;
    const auto invalid_schedule = make_binary_snowball_option(mismatched);
    REQUIRE_FALSE(invalid_schedule);
    CHECK(invalid_schedule.error().category == ErrorCategory::invalid_schedule);
}

TEST_CASE("Autocallable convenience getters own their observation values", "[observation-ownership]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), first = day(2025, 7, 1), end = day(2026, 1, 1);
    std::vector<Date> dates;
    std::vector<double> levels, coupons, barriers;
    {
        const auto snowball = *make_binary_snowball_option(
            {.maturity_coupon_rate = 0.05, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.08}, {end, 110.0, 0.12}}});
        const auto phoenix = *make_phoenix_option(
            {.coupon_rate = 0.08, .initial_spot = 100.0, .knock_in_level = 75.0, .upper_strike = 100.0, .lower_strike = 0.0, .effective_date = start, .expiry_date = end, .observations = {{first, 115.0, 0.0, 85.0}, {end, 110.0, 0.0, 80.0}}});
        dates = snowball.observation_dates();
        levels = snowball.knock_out_levels();
        coupons = snowball.knock_out_coupon_rates();
        barriers = phoenix.coupon_barrier_levels();
        const auto iterator = snowball.observations_view().begin();
        CHECK(iterator->date == first);
        auto changed = snowball.knock_out_levels();
        changed.front() = 1.0;
        CHECK(snowball.observations_view().front().knock_out_level == 115.0);
    }
    CHECK(dates == std::vector<Date>{first, end});
    CHECK(levels == std::vector<double>{115.0, 110.0});
    CHECK(coupons == std::vector<double>{0.08, 0.12});
    CHECK(barriers == std::vector<double>{85.0, 80.0});
}

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

struct CopyCountingObservations : std::vector<kiyosi::AutocallableObservation> {
    int* copies;
    CopyCountingObservations(int& count, kiyosi::Date expiry)
        : std::vector<kiyosi::AutocallableObservation>{{expiry, 110.0, 0.05}}, copies(&count) {}
    CopyCountingObservations(const CopyCountingObservations& other)
        : std::vector<kiyosi::AutocallableObservation>(other), copies(other.copies) { ++*copies; }
    ~CopyCountingObservations() = default;
    CopyCountingObservations& operator=(const CopyCountingObservations&) = delete;
    CopyCountingObservations(CopyCountingObservations&&) = delete;
    CopyCountingObservations& operator=(CopyCountingObservations&&) = delete;
};
} // namespace

static_assert(std::is_copy_constructible_v<kiyosi::SnowballOption>);
static_assert(std::is_copy_assignable_v<kiyosi::SnowballOption>);
static_assert(std::is_move_constructible_v<kiyosi::SnowballOption>);
static_assert(std::is_move_assignable_v<kiyosi::SnowballOption>);
static_assert(std::is_copy_constructible_v<kiyosi::BinarySnowballOption>);
static_assert(std::is_copy_assignable_v<kiyosi::BinarySnowballOption>);
static_assert(std::is_move_constructible_v<kiyosi::BinarySnowballOption>);
static_assert(std::is_move_assignable_v<kiyosi::BinarySnowballOption>);
static_assert(std::is_copy_constructible_v<kiyosi::TernarySnowballOption>);
static_assert(std::is_copy_assignable_v<kiyosi::TernarySnowballOption>);
static_assert(std::is_move_constructible_v<kiyosi::TernarySnowballOption>);
static_assert(std::is_move_assignable_v<kiyosi::TernarySnowballOption>);
static_assert(std::is_copy_constructible_v<kiyosi::PhoenixOption>);
static_assert(std::is_copy_assignable_v<kiyosi::PhoenixOption>);
static_assert(std::is_move_constructible_v<kiyosi::PhoenixOption>);
static_assert(std::is_move_assignable_v<kiyosi::PhoenixOption>);

template <typename Note>
concept HasParticipationTerms = requires(const Note& note) {
    note.initial_spot();
    note.upper_strike();
    note.lower_strike();
};
static_assert(!HasParticipationTerms<kiyosi::BinarySnowballOption>);
static_assert(!HasParticipationTerms<kiyosi::TernarySnowballOption>);
static_assert(HasParticipationTerms<kiyosi::SnowballOption>);
static_assert(HasParticipationTerms<kiyosi::PhoenixOption>);

TEST_CASE("Complete autocallables validate their product-specific terms", "[autocallable-terms]")
{
    const auto effective = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto binary = kiyosi::make_binary_snowball_option(
        {.maturity_coupon_rate = 0.05, .effective_date = effective, .expiry_date = expiry, .observations = {{expiry, 110.0, 0.1}}});
    REQUIRE(binary);
    CHECK(kiyosi::validate_autocallable_note(*binary));

    const auto ternary = kiyosi::make_ternary_snowball_option(
        {.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.0, .knock_in_level = 70.0, .barrier_state = kiyosi::AutocallableBarrierState::knocked_in, .effective_date = effective, .expiry_date = expiry, .observations = {{expiry, 110.0, 0.1}}});
    REQUIRE(ternary);
    CHECK(kiyosi::validate_autocallable_note(*ternary));

    const auto invalid_binary = kiyosi::make_binary_snowball_option(
        {.maturity_coupon_rate = 0.05, .barrier_state = kiyosi::AutocallableBarrierState::knocked_in, .effective_date = effective, .expiry_date = expiry, .observations = {{expiry, 110.0, 0.1}}});
    REQUIRE_FALSE(invalid_binary);
    CHECK(invalid_binary.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("Autocallable validation does not copy its input")
{
    int copies = 0;
    const struct Terms {
        double principal_ratio;
        kiyosi::Date effective_date;
        kiyosi::Date expiry_date;
        std::optional<kiyosi::AutocallableBarrierState> barrier_state;
        double maturity_coupon_rate;
        CopyCountingObservations observations;
    } terms{1.0, day(2025, 1, 1), day(2026, 1, 1), kiyosi::AutocallableBarrierState::none,
            0.05, CopyCountingObservations{copies, day(2026, 1, 1)}};

    REQUIRE(kiyosi::detail::validate_autocallable_terms(terms));
    CHECK(copies == 0);
}

TEST_CASE("Autocallable factories own copied or moved construction terms", "[autocallable-terms]")
{
    using namespace kiyosi;
    const auto start = day(2025, 1, 1), end = day(2026, 1, 1);
    const auto check = [](auto factory, auto terms) {
        const auto copy = factory(terms);
        REQUIRE(copy);
        const auto original = terms.observations.front();
        terms.observations.front().knock_out_level += 1.0;
        CHECK(copy->observations_view().front() == original);
        const auto* storage = terms.observations.data();
        auto moved = factory(std::move(terms));
        REQUIRE(moved);
        CHECK(moved->observations_view().data() == storage);
        CHECK(*copy != *moved);
        CHECK(validate_autocallable_note(*moved));
        auto assigned = *copy;
        assigned = *moved;
        CHECK(assigned == *moved);
        CHECK(assigned.observations_view().data() != moved->observations_view().data());
        const auto& same = assigned;
        assigned = same;
        CHECK(assigned == *moved);
        const auto owner = std::move(*moved);
        CHECK(validate_autocallable_note(owner));
        const auto invalid = validate_autocallable_note(*moved); // NOLINT(bugprone-use-after-move): verifies moved-from rejection.
        REQUIRE_FALSE(invalid);
        CHECK(invalid.error().category == ErrorCategory::invalid_schedule);
    };
    check(make_binary_snowball_option, BinarySnowballTerms{.maturity_coupon_rate = 0.05,
                                                           .effective_date = start,
                                                           .expiry_date = end,
                                                           .observations = {{end, 110.0, 0.05}}});
    check(make_ternary_snowball_option, TernarySnowballTerms{.maturity_coupon_rate = 0.05,
                                                             .minimum_coupon_rate = 0.01,
                                                             .knock_in_level = 80.0,
                                                             .effective_date = start,
                                                             .expiry_date = end,
                                                             .observations = {{end, 110.0, 0.05}}});
    check(make_snowball_option, SnowballTerms{.maturity_coupon_rate = 0.05,
                                              .initial_spot = 100.0,
                                              .knock_in_level = 80.0,
                                              .upper_strike = 100.0,
                                              .lower_strike = 60.0,
                                              .effective_date = start,
                                              .expiry_date = end,
                                              .observations = {{end, 110.0, 0.05}}});
    check(make_phoenix_option, PhoenixTerms{.coupon_rate = 0.05,
                                            .initial_spot = 100.0,
                                            .knock_in_level = 80.0,
                                            .upper_strike = 100.0,
                                            .lower_strike = 60.0,
                                            .effective_date = start,
                                            .expiry_date = end,
                                            .observations = {{end, 110.0, 0.0, 80.0}}});
}

TEST_CASE("Autocallable factories preserve validation error categories")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto check = [](auto factory, auto terms) {
        if constexpr (requires { terms.initial_spot; }) terms.initial_spot = 0.0;
        else terms.principal_ratio = -1.0;
        const auto invalid_parameter = factory(terms);
        REQUIRE_FALSE(invalid_parameter.has_value());
        CHECK(invalid_parameter.error().category == kiyosi::ErrorCategory::invalid_parameter);
        if constexpr (requires { terms.initial_spot; }) terms.initial_spot = 100.0;
        else terms.principal_ratio = 1.0;
        terms.observations.clear();
        const auto invalid_schedule = factory(std::move(terms));
        REQUIRE_FALSE(invalid_schedule.has_value());
        CHECK(invalid_schedule.error().category == kiyosi::ErrorCategory::invalid_schedule);
    };

    check(kiyosi::make_phoenix_option,
          kiyosi::PhoenixTerms{.coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 60.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.0, 80.0}}});
    check(kiyosi::make_binary_snowball_option,
          kiyosi::BinarySnowballTerms{.maturity_coupon_rate = 0.05, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.05}}});
    const kiyosi::BinarySnowballTerms binary_terms{.maturity_coupon_rate = 0.05, .barrier_state = kiyosi::AutocallableBarrierState::knocked_in, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.05}}};
    const auto invalid_binary = kiyosi::make_binary_snowball_option(binary_terms);
    REQUIRE_FALSE(invalid_binary);
    CHECK(invalid_binary.error().category == kiyosi::ErrorCategory::invalid_parameter);
    auto knocked_out_binary = binary_terms;
    knocked_out_binary.barrier_state = kiyosi::AutocallableBarrierState::knocked_out;
    const auto knocked_out = kiyosi::make_binary_snowball_option(knocked_out_binary);
    CHECK(knocked_out.has_value());
    check(kiyosi::make_snowball_option,
          kiyosi::SnowballTerms{.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 60.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.05}}});
    check(kiyosi::make_ternary_snowball_option,
          kiyosi::TernarySnowballTerms{.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.01, .knock_in_level = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.05}}});
}

TEST_CASE("Snowball factory rejects invalid schedules and accepts signed coupons")
{
    static_assert(!std::is_constructible_v<kiyosi::BinarySnowballOption, std::vector<double>, double, double,
                                           std::vector<double>, double, double, std::vector<kiyosi::Date>,
                                           kiyosi::AutocallableBarrierState, double, kiyosi::Date, kiyosi::Date>);

    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto note = kiyosi::make_snowball_option(kiyosi::SnowballTerms{.maturity_coupon_rate = -0.05, .initial_spot = 100.0, .knock_in_level = 60.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, -0.1}}});
    REQUIRE(note);
    const auto invalid_schedule = kiyosi::make_snowball_option(kiyosi::SnowballTerms{.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 60.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 110.0, 0.1}, {effective_date, 110.0, 0.1}}});
    REQUIRE_FALSE(invalid_schedule.has_value());
    CHECK(invalid_schedule.error().category == kiyosi::ErrorCategory::invalid_schedule);
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
    const auto binary = kiyosi::make_binary_snowball_option({.maturity_coupon_rate = 0.03, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observation_dates, std::vector<double>{105.0, 100.0}, std::vector<double>{0.10, 0.09})});
    const auto ternary = kiyosi::make_ternary_snowball_option({.maturity_coupon_rate = 0.03, .minimum_coupon_rate = 0.01, .knock_in_level = 70.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = kiyosi::AutocallableBarrierState::none, .principal_ratio = 1.0, .effective_date = effective_date, .expiry_date = expiry_date, .observations = *kiyosi::make_autocallable_observations(observation_dates, std::vector<double>{105.0, 100.0}, std::vector<double>{0.10, 0.09})});
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

TEST_CASE("Ternary coupon floor cannot exceed its maturity coupon")
{
    const auto start = day(2025, 1, 1);
    const auto expiry = day(2026, 1, 1);
    const auto invalid = kiyosi::make_ternary_snowball_option(
        {.maturity_coupon_rate = 0.05, .minimum_coupon_rate = 0.06, .knock_in_level = 80.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::at_expiry, .effective_date = start, .expiry_date = expiry, .observations = {{expiry, 120.0, 0.1}}});
    REQUIRE_FALSE(invalid);
    CHECK(invalid.error().category == kiyosi::ErrorCategory::invalid_parameter);
}

TEST_CASE("Autocallable valuation requires possible explicit history")
{
    const auto effective = day(2025, 1, 1);
    const auto first = day(2025, 1, 2);
    const auto expiry = day(2025, 1, 6);
    const auto market = [&](kiyosi::Date valuation) {
        // NOLINTNEXTLINE(clang-analyzer-core.StackAddressEscape): PricingContext is returned by value.
        const auto parameters = kiyosi::make_bsm_parameters(0.05, 0.02, 0.2);
        REQUIRE(parameters);
        const auto result = kiyosi::make_pricing_context(*parameters, 100.0, valuation);
        REQUIRE(result);
        return *result;
    };
    const auto note = [&](std::optional<kiyosi::AutocallableBarrierState> state) {
        const auto result = kiyosi::make_snowball_option(
            {.maturity_coupon_rate = 0.05, .initial_spot = 100.0, .knock_in_level = 80.0, .upper_strike = 100.0, .lower_strike = 60.0, .knock_in_observation_mode = kiyosi::KnockInObservationMode::every_trading_day, .barrier_state = state, .effective_date = effective, .expiry_date = expiry, .observations = {{first, 120.0, 0.1}, {expiry, 120.0, 0.1}}});
        REQUIRE(result);
        return *result;
    };
    const kiyosi::MonteCarloAutocallableEngine mc{{32, 1}};
    const kiyosi::FiniteDifferenceAutocallableEngine fd{{40, 40}};
    CHECK(mc.price(note(std::nullopt), market(effective)).has_value());
    const auto missing = mc.price(note(std::nullopt), market(day(2025, 1, 3)));
    REQUIRE_FALSE(missing);
    CHECK(missing.error().category == kiyosi::ErrorCategory::invalid_parameter);
    const auto missing_fd = fd.price(note(std::nullopt), market(day(2025, 1, 3)));
    REQUIRE_FALSE(missing_fd.has_value());
    CHECK(missing_fd.error().category ==
          kiyosi::ErrorCategory::invalid_parameter);
    CHECK(mc.price(note(kiyosi::AutocallableBarrierState::none), market(day(2025, 1, 3))).has_value());
    const auto premature = mc.price(note(kiyosi::AutocallableBarrierState::knocked_out), market(effective));
    REQUIRE_FALSE(premature);
    CHECK(premature.error().category == kiyosi::ErrorCategory::invalid_option);
    const auto at_first = mc.price(note(kiyosi::AutocallableBarrierState::knocked_out), market(first));
    REQUIRE_FALSE(at_first);
    CHECK(at_first.error().category == kiyosi::ErrorCategory::invalid_option);
    const auto after_first = *kiyosi::make_pricing_context(
        market(first).model_parameters(), 100.0, kiyosi::start_of_day(first) + std::chrono::microseconds{1});
    const auto settled_first = mc.price(note(kiyosi::AutocallableBarrierState::knocked_out), after_first);
    REQUIRE(settled_first);
    CHECK(*settled_first == 0.0);
    const auto settled = mc.price(note(kiyosi::AutocallableBarrierState::knocked_out), market(day(2025, 1, 3)));
    REQUIRE(settled);
    CHECK(*settled == 0.0);
}

TEST_CASE("Negative Binary Snowball coupon can be implied")
{
    const auto effective_date = day(2025, 1, 1);
    const auto expiry_date = day(2026, 1, 1);
    const auto option = kiyosi::make_binary_snowball_option({.maturity_coupon_rate = -0.1, .effective_date = effective_date, .expiry_date = expiry_date, .observations = {{expiry_date, 1.0, -0.1}}});
    REQUIRE(option);
    const auto context = *kiyosi::make_pricing_context(
        *kiyosi::make_bsm_parameters(0.05, 0.02, 0.2), 100.0, effective_date);
    const kiyosi::MonteCarloAutocallableEngine engine{{32, 73}};
    const auto price = engine.price(*option, context);
    REQUIRE(price);
    const auto implied = kiyosi::implied_coupon(engine, *option, context, *price,
                                                kiyosi::CouponQuoteConvention::preserve_maturity_coupon, {-0.2, 0.2});
    REQUIRE(implied);
    CHECK(*implied == Catch::Approx(-0.1).epsilon(0.0).margin(1e-7));
}
