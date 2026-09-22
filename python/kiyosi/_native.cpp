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

NB_MODULE(_native, module)
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

    bind_enums(module);
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
    const nb::object exception_type = nb::steal<nb::object>(
        PyErr_NewException("kiyosi.KiyosiError", PyExc_Exception,
                           exception_attributes.ptr()));
    module.attr("KiyosiError") = exception_type;
    nb::register_exception_translator(translate_domain_exception, exception_type.ptr());

    bind_market(module);
    bind_instruments(module);
    bind_results(module);
    bind_engines(module);
    bind_analytics(module);
}
