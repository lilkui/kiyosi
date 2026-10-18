#include "_binding.hpp"

#include <exception>
#include <string>

namespace kiyosi::python_binding {

namespace {

void translate_domain_exception(const std::exception_ptr& pointer, void* payload)
{
    try {
        if (pointer) std::rethrow_exception(pointer);
    } catch (const DomainException& error) {
        const nb::object exception_type = nb::borrow<nb::object>(static_cast<PyObject*>(payload));
        nb::object instance = exception_type(error.what());
        instance.attr("category") = nb::cast(error.category());
        PyErr_SetObject(exception_type.ptr(), instance.ptr());
    }
}

} // namespace

} // namespace kiyosi::python_binding

NB_MODULE(_native, module) // NOLINT(cppcoreguidelines-pro-type-vararg,cppcoreguidelines-avoid-non-const-global-variables,cppcoreguidelines-pro-type-cstyle-cast)
{
    using namespace kiyosi;
    using namespace kiyosi::python_binding;

    module.doc() = R"doc(Native domain and pricing API for kiyosi.

The public Python modules re-export the supported symbols from this extension.
Representation failures use Python's built-in exceptions; domain rejections
raise :class:`KiyosiError` with a stable :attr:`KiyosiError.category`.)doc";
    const std::string version = std::to_string(version_major) + "." +
                                std::to_string(version_minor) + "." +
                                std::to_string(version_patch);
    module.attr("__version__") = nb::str(version.c_str());

    bind_choice<OptionType>(module, "OptionType");
    bind_choice<BarrierType>(module, "BarrierType");
    bind_choice<ObservationMode>(module, "ObservationMode");
    bind_choice<BarrierTouchState>(module, "BarrierTouchState");
    bind_choice<RebateTiming>(module, "RebateTiming");
    bind_choice<SettlementTiming>(module, "SettlementTiming");
    bind_choice<PayoffType>(module, "PayoffType");
    bind_choice<KnockInObservationMode>(module, "KnockInObservationMode");
    bind_choice<AutocallableBarrierState>(module, "AutocallableBarrierState");
    bind_choice<FiniteDifferenceScheme>(module, "FiniteDifferenceScheme");
    bind_choice<MonteCarloBackend>(module, "MonteCarloBackend");
    bind_choice<CouponQuoteConvention>(module, "CouponQuoteConvention");
    bind_choice<BusinessDayConvention>(module, "BusinessDayConvention");
    bind_choice<Greek>(module, "Greek");

    bind_error_category(module);
    nb::dict exception_attributes;
    exception_attributes["__doc__"] = R"doc(Core domain validation or pricing error.

Parameters
----------
message : str
    Human-readable diagnostic message. Callers should branch on ``category``
    rather than parse this text.

Attributes
----------
category : ErrorCategory
    Stable machine-readable error category.)doc";
    nb::object exception_type = nb::steal<nb::object>(
        PyErr_NewException("kiyosi.KiyosiError", PyExc_Exception,
                           exception_attributes.ptr()));
    module.attr("KiyosiError") = exception_type;
    // Keep the translator's class alive even if its public exports are removed.
    const nb::capsule exception_owner(exception_type.ptr(), [](void* pointer) noexcept {
        Py_DECREF(static_cast<PyObject*>(pointer));
    });
    exception_type.inc_ref();
    module.attr("_domain_exception_owner") = exception_owner;
    nb::register_exception_translator(translate_domain_exception, exception_type.ptr());

    bind_market(module);
    bind_instruments(module);
    bind_results(module);
    bind_engines(module);
    bind_analytics(module);
}
