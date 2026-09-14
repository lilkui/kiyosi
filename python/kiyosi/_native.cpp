#include <nanobind/nanobind.h>
#include <nanobind/stl/chrono.h>
#include <nanobind/stl/string.h>

#include <chrono>
#include <string>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/engines/vanilla/analytic.hpp>

namespace nb = nanobind;
using namespace kiyosi;

namespace {

date make_date(int year, int month, int day)
{
    return date{std::chrono::year{year} / std::chrono::month{static_cast<unsigned>(month)} /
                std::chrono::day{static_cast<unsigned>(day)}};
}

nb::dict error_result(const Error& error)
{
    nb::dict output;
    output["__kiyosi_error__"] = true;
    output["category"] = error.category;
    output["message"] = error.message;
    return output;
}

void set_value(nb::dict& output, const char* name, const PricingResult& result, risk_measure measure)
{
    if (const auto value = result.get(measure))
        output[name] = *value;
    else
        output[name] = nb::none();
}

nb::dict price(
    const std::string& kind, double spot, double strike,
    date valuation, date expiry,
    double risk_free_rate, double dividend_yield, double volatility,
    date effective)
{
    result<EuropeanOption> option = std::unexpected(
        Error{error_category::invalid_option, "kind must be 'call' or 'put'"});
    if (kind == "call") {
        option = make_european_option(option_type::call, strike, effective, expiry);
    } else if (kind == "put") {
        option = make_european_option(option_type::put, strike, effective, expiry);
    } else {
        return error_result(option.error());
    }
    if (!option) return error_result(option.error());

    auto parameters = make_bsm_parameters(risk_free_rate, dividend_yield, volatility);
    if (!parameters) return error_result(parameters.error());
    auto context = make_pricing_context(*parameters, spot, valuation);
    if (!context) return error_result(context.error());

    const auto priced = AnalyticVanillaEngine{}.price(*option, *context);
    if (!priced) return error_result(priced.error());

    nb::dict output;
    set_value(output, "price", *priced, risk_measure::price);
    set_value(output, "delta", *priced, risk_measure::delta);
    set_value(output, "gamma", *priced, risk_measure::gamma);
    set_value(output, "theta", *priced, risk_measure::theta);
    set_value(output, "vega", *priced, risk_measure::vega);
    set_value(output, "rho", *priced, risk_measure::rho);
    return output;
}

} // namespace

NB_MODULE(_native, module)
{
    module.doc() = "Native closed-form pricing primitives for kiyosi.";
    nb::enum_<error_category>(module, "ErrorCategory")
        .value("INVALID_OPTION", error_category::invalid_option)
        .value("INVALID_STRIKE", error_category::invalid_strike)
        .value("INVALID_VOLATILITY", error_category::invalid_volatility)
        .value("INVALID_RATE", error_category::invalid_rate)
        .value("INVALID_DIVIDEND", error_category::invalid_dividend)
        .value("INVALID_ASSET_PRICE", error_category::invalid_asset_price)
        .value("INVALID_DATE", error_category::invalid_date)
        .value("INVALID_EXPIRY", error_category::invalid_expiry)
        .value("INVALID_RESULT", error_category::invalid_result)
        .value("INVALID_SCHEDULE", error_category::invalid_schedule)
        .value("INVALID_CALENDAR", error_category::invalid_calendar)
        .value("INVALID_PARAMETER", error_category::invalid_parameter)
        .value("INVALID_QUOTE", error_category::invalid_quote)
        .value("UNBRACKETED_VOLATILITY", error_category::unbracketed_volatility)
        .value("SOLVER_NON_CONVERGENCE", error_category::solver_non_convergence)
        .value("SOLVER_NON_FINITE", error_category::solver_non_finite)
        .value("UNBRACKETED_COUPON", error_category::unbracketed_coupon);
    module.def("price", &price);
}
