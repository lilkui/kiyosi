#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <cmath>
#include <filesystem>
#include <set>
#include <string>
#include <type_traits>

#include <kiyosi/kiyosi.hpp>

#include "support/reference_fixture.hpp"

namespace {
using namespace kiyosi;
using test::ReferenceCase;

double number(const ReferenceCase& row, const std::string& key)
{
    return test::detail::number(row.inputs.at(key), 0, key);
}

Date date(const ReferenceCase& row, const std::string& key)
{
    return test::detail::calendar_date(row.inputs.at(key), 0, key);
}

std::vector<ReferenceCase> references()
{
    return test::load_reference_cases(std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests/fixtures/structured_reference.tsv");
}

PricingContext market(const ReferenceCase& row)
{
    const auto context = make_pricing_context(*make_bsm_parameters(number(row, "rate"), number(row, "dividend"), number(row, "volatility")), number(row, "spot"),
                                              start_of_day(date(row, "valuation")) + std::chrono::hours{static_cast<int>(number(row, "valuation_hour"))}, weekdays_calendar());
    REQUIRE(context);
    return *context;
}

template <typename Terms>
Terms terms(const ReferenceCase& row)
{
    Terms result{};
    result.principal_ratio = number(row, "principal_ratio");
    result.effective_date = date(row, "effective_date");
    result.expiry_date = date(row, "expiry_date");
    result.barrier_state = row.inputs.at("history") == "knocked_in" ? AutocallableBarrierState::knocked_in : AutocallableBarrierState::none;
    if constexpr (requires { result.knock_in_level; }) {
        result.knock_in_level = number(row, "knock_in_level");
        result.knock_in_observation_mode = row.inputs.at("knock_in_mode") == "at_expiry" ? KnockInObservationMode::at_expiry : KnockInObservationMode::every_trading_day;
    }
    if constexpr (requires { result.initial_spot; }) {
        result.initial_spot = number(row, "initial_spot");
        result.upper_strike = number(row, "upper_strike");
        result.lower_strike = number(row, "lower_strike");
    }
    if constexpr (requires { result.maturity_coupon_rate; }) result.maturity_coupon_rate = number(row, "maturity_coupon_rate");
    if constexpr (requires { result.minimum_coupon_rate; }) result.minimum_coupon_rate = number(row, "minimum_coupon_rate");
    if constexpr (std::is_same_v<Terms, PhoenixTerms>) {
        result.coupon_rate = number(row, "coupon_rate");
        result.observations = {{result.expiry_date, number(row, "knock_out_level"), 0.0, number(row, "coupon_barrier_level")}};
    } else {
        result.observations = {{result.expiry_date, number(row, "knock_out_level"), number(row, "coupon_rate")}};
    }
    return result;
}

void with_contract(const ReferenceCase& row, const auto& check)
{
    if (row.instrument == "Accumulator")
        check(make_accumulator({number(row, "strike"), number(row, "knock_out_level"), number(row, "daily_quantity"), number(row, "acceleration_factor"), number(row, "accumulated_quantity"), date(row, "effective_date"), date(row, "expiry_date")}).value());
    else if (row.instrument == "SnowballOption") check(make_snowball_option(terms<SnowballTerms>(row)).value());
    else if (row.instrument == "BinarySnowballOption") check(make_binary_snowball_option(terms<BinarySnowballTerms>(row)).value());
    else if (row.instrument == "TernarySnowballOption") check(make_ternary_snowball_option(terms<TernarySnowballTerms>(row)).value());
    else if (row.instrument == "PhoenixOption") check(make_phoenix_option(terms<PhoenixTerms>(row)).value());
    else FAIL("Unknown structured instrument: " << row.instrument);
}

FiniteDifferenceSettings settings(const ReferenceCase& row)
{
    return {static_cast<int>(number(row, "asset_step_count")), static_cast<int>(number(row, "time_step_count")), FiniteDifferenceScheme::crank_nicolson, number(row, "asset_upper_boundary")};
}

auto finite_difference(const auto& option, const PricingContext& context, FiniteDifferenceSettings configuration)
{
    if constexpr (std::is_same_v<std::remove_cvref_t<decltype(option)>, Accumulator>)
        return make_finite_difference_accumulator_engine(configuration).value().price(option, context);
    else return make_finite_difference_autocallable_engine(configuration).value().price(option, context);
}

} // namespace

TEST_CASE("Structured terminal prices match independent QuantLib payoff portfolios", "[structured-reference]")
{
    std::set<std::string> instruments;
    bool signed_value = false;
    for (const auto& row : references()) {
        INFO(row.case_id);
        instruments.insert(row.instrument);
        REQUIRE(row.provenance.source_symbol == "QuantLib.BlackCalculator");
        REQUIRE(row.inputs.at("reference_classification") == "quantlib-analytic-portfolio");
        REQUIRE(row.inputs.at("calendar") == "weekends_only");
        REQUIRE(row.outputs.size() == 1);
        signed_value = signed_value || row.outputs.at("price") < 0.0;
        const auto context = market(row);
        with_contract(row, [&](const auto& option) {
            const auto actual = [&] {
                if (row.engine.starts_with("FiniteDifference")) return finite_difference(option, context, settings(row));
                if constexpr (std::is_same_v<std::remove_cvref_t<decltype(option)>, Accumulator>)
                    return make_quadrature_accumulator_engine({static_cast<int>(number(row, "asset_step_count")), number(row, "integration_standard_deviations")}).value().price(option, context);
                else return make_quadrature_autocallable_engine({static_cast<int>(number(row, "asset_step_count")), number(row, "integration_standard_deviations")}).value().price(option, context);
            }();
            if (!actual) INFO(actual.error().message);
            REQUIRE(actual);
            CAPTURE(*actual, row.outputs.at("price"));
            CHECK(std::abs(*actual - row.outputs.at("price")) <= row.tolerances.at("price") + number(row, "uncertainty_price"));
        });
    }
    CHECK(instruments == std::set<std::string>{"Accumulator", "SnowballOption", "BinarySnowballOption", "TernarySnowballOption", "PhoenixOption"});
    CHECK(signed_value);
}

TEST_CASE("Structured finite differences converge separately in space time and domain", "[structured-convergence]")
{
    int compared = 0;
    for (const auto& row : references()) {
        if (!row.engine.starts_with("FiniteDifference") || row.inputs.at("convergence") != "true") continue;
        INFO(row.case_id);
        ++compared;
        const auto context = market(row);
        const auto fine = settings(row);
        auto coarse_space = fine, coarse_time = fine, wide = fine;
        coarse_space.asset_step_count /= 2;
        coarse_time.time_step_count /= 2;
        wide.asset_step_count *= 2;
        wide.asset_upper_boundary = *fine.asset_upper_boundary * 2.0;
        const double budget = row.tolerances.at("price") + number(row, "uncertainty_price");
        with_contract(row, [&](const auto& option) {
            const auto refined = finite_difference(option, context, fine);
            REQUIRE(refined);
            CHECK(std::abs(*refined - row.outputs.at("price")) <= budget);
            for (const auto configuration : {coarse_space, coarse_time, wide}) {
                CAPTURE(configuration.asset_step_count, configuration.time_step_count, configuration.asset_upper_boundary);
                const auto value = finite_difference(option, context, configuration);
                REQUIRE(value);
                CHECK(std::abs(*value - *refined) <= budget);
            }
        });
    }
    CHECK(compared == 7);
}
