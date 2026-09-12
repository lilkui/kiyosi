#include <nanobind/nanobind.h>
#include <nanobind/stl/string.h>

#include <chrono>
#include <string>

#include <kiyosi/instruments/vanilla.hpp>
#include <kiyosi/market/context.hpp>
#include <kiyosi/pricing/engines/vanilla/analytic.hpp>

namespace nb = nanobind;
using namespace kiyosi;

namespace {

[[noreturn]] void raise_error(const Error& error)
{
    throw nb::value_error(error.message.c_str());
}

date make_date(int year, int month, int day)
{
    return date{std::chrono::year{year} / std::chrono::month{static_cast<unsigned>(month)} /
                std::chrono::day{static_cast<unsigned>(day)}};
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
    int valuation_year, int valuation_month, int valuation_day,
    int expiry_year, int expiry_month, int expiry_day,
    double risk_free_rate, double dividend_yield, double volatility)
{
    const auto valuation = make_date(valuation_year, valuation_month, valuation_day);
    const auto expiry = make_date(expiry_year, expiry_month, expiry_day);

    result<EuropeanOption> option = kind == "call"
        ? make_european_call(strike, valuation, expiry)
        : kind == "put" ? make_european_put(strike, valuation, expiry)
                         : result<EuropeanOption>{std::unexpected(
                               Error{error_category::invalid_option, "kind must be 'call' or 'put'"})};
    if (!option) raise_error(option.error());

    auto parameters = make_bsm_parameters(risk_free_rate, dividend_yield, volatility);
    if (!parameters) raise_error(parameters.error());
    auto asset_price = make_asset_price(spot);
    if (!asset_price) raise_error(asset_price.error());
    auto context = make_pricing_context(*parameters, *asset_price, valuation);
    if (!context) raise_error(context.error());

    const auto priced = AnalyticEuropeanEngine{}.price(*option, *context);
    if (!priced) raise_error(priced.error());

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
    module.def("price", &price);
}
