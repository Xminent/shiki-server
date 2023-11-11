mod opcode;

use syn::DeriveInput;

#[proc_macro_derive(HasOpcode, attributes(opcode))]
pub fn opcode_derive_opcode(
	input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
	let ast: DeriveInput = syn::parse(input).unwrap();

	opcode::expand(&ast).into()
}
