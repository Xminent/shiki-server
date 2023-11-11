use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};

const OPCODE_ATTR: &str = "opcode";

pub fn expand(ast: &syn::DeriveInput) -> TokenStream {
	let item_value = match get_attribute_value_multiple(ast, OPCODE_ATTR) {
		Ok(val) => match val.len() {
			1 => val[0].clone(),
			_ => {
				return syn::Error::new(
					Span::call_site(),
					format!(
						"#[{}(value)] takes 1 parameters, given {}",
						OPCODE_ATTR,
						val.len()
					),
				)
				.to_compile_error()
			}
		},
		Err(err) => return err.to_compile_error(),
	};

	let name = &ast.ident;
	let (impl_generics, ty_generics, where_clause) =
		ast.generics.split_for_impl();

	let item_value = item_value
		.map(ToTokens::into_token_stream)
		.unwrap_or_else(|| quote! { () });

	quote! {
		impl #impl_generics HasOpcode for #name #ty_generics #where_clause {
			fn opcode() -> Opcode {
				#item_value
			}
		}
	}
}

fn get_attribute_value_multiple(
	ast: &syn::DeriveInput, name: &str,
) -> syn::Result<Vec<Option<syn::Type>>> {
	let attr = ast
		.attrs
		.iter()
		.find_map(|attr| {
			if attr.path().is_ident(name) {
				attr.parse_args().ok()
			} else {
				None
			}
		})
		.ok_or_else(|| {
			syn::Error::new(
				Span::call_site(),
				format!("Expect an attribute `{name}`"),
			)
		})?;

	match attr {
		syn::Meta::NameValue(ref nv) => match nv.path.get_ident() {
			Some(ident) if ident == "value" => {
				if let syn::Expr::Lit(syn::ExprLit {
					lit: syn::Lit::Str(lit),
					..
				}) = nv.value.clone()
				{
					if let Ok(ty) = syn::parse_str::<syn::Type>(&lit.value()) {
						return Ok(vec![Some(ty)]);
					}
				}
				Err(syn::Error::new_spanned(&nv.value, "Expect type"))
			}
			_ => Err(syn::Error::new_spanned(
				&nv.value,
				r#"Expect `value = "TYPE"`"#,
			)),
		},

		_ => Err(syn::Error::new_spanned(&attr, "Expect a string literal")),
	}
}
