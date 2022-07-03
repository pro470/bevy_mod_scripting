use std::{io::{self, BufReader},fs::{File,read_to_string}, collections::{HashSet, BTreeMap}};
use clap::Parser;
use indexmap::IndexMap;
use serde_json::from_reader;
use rustdoc_types::{Crate, Item, ItemEnum, Id, Impl,Type};
use serde_derive::Deserialize;


static WRAPPER_PREFIX : &'static str = "Lua";

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {

    /// Paths to json files generated by `rustdoc -p <crate> --output-format json`
    #[clap(short, long, value_parser)]
    json: Vec<String>,
    

    /// The path to toml config file which contains the types to be wrapped and overrides
    #[clap(short, long, value_parser)]
    config: String,
}

#[derive(Deserialize,Debug)]
pub(crate) struct Config {
    #[serde(skip_deserializing,default)]
    pub types : IndexMap<String,Newtype>,

    #[serde(rename="types")]
    pub types_ : Vec<Newtype>,

    pub imports : String,

    pub external_types : Vec<String>,

    pub primitives : HashSet<String>,
}




#[derive(Deserialize,Debug)]
pub(crate) struct Newtype {

    #[serde(rename="type")]
    type_ : String,

    /// Override type-level docstring 
    pub doc : Option<String>,

    #[serde(default)]
    pub source : Source,

    #[serde(default)]
    pub wrapper_type : WrapperType,

    #[serde(default)]
    pub lua_methods: Vec<String>,

    #[serde(default)]
    pub derive_flags: Vec<String>,

    #[serde(default)]
    pub import_path: String,
}

#[derive(Deserialize,Debug)]
pub(crate) struct Source(String);

impl Default for Source {
    fn default() -> Self {
        Self("bevy".to_string())
    }
}

impl Newtype {
    /// Returns true if this Type:
    /// - describes the given item element
    /// - if the element is fully described in the source crate
    /// - if the element is a struct or enum
    pub fn matches_result(&self, item : &Item, source : &Crate) -> bool {
        
        match &item.inner {
            ItemEnum::Struct(s) => {},
            ItemEnum::Enum(e) => {},
            _ => return false
        };

        if source.external_crates.contains_key(&item.crate_id){
            return false
        };

        true
    }
}


#[derive(Deserialize, Debug,Clone, Copy)]
pub(crate) enum WrapperType {
    /// things which can be freely assigned to with reflect
    Reflect,
    /// For things without reflect impls
    NonReflect,
    /// For primitives
    Primitive
}

impl ToString for WrapperType {
    fn to_string(&self) -> String {
        match self {
            WrapperType::Reflect => "Reflect".to_string(),
            WrapperType::NonReflect => "NonReflect".to_string(),
            WrapperType::Primitive => "Primitive".to_string(),
        }
    }
}

impl Default for WrapperType {
    fn default() -> Self {
        Self::Reflect
    }
}

#[derive(Debug)]
pub(crate) struct WrappedItem<'a> {
    wrapper_type : WrapperType,
    wrapper_name : String,
    wrapped_type: &'a String,
    path_components: &'a [String],
    source: &'a Crate,
    config: &'a Newtype,
    item : &'a Item,
    /// The items coming from all trait implementations
    impl_items: IndexMap<&'a str,Vec<(&'a Impl, &'a Item)>>, 

    self_impl: Option<&'a Impl>,
}


pub(crate) fn is_valid_lua_fn_arg(str : &str, config : &Config) -> bool{
    const FROM_PRIMITIVES : [&str;19] = ["bool","StdString","Box<str>","CString","BString","i8","u8","i16","u16","i32","u32","i64","u64","i128","u128","isize","usize","f32","f64"];

    // we also allow references to these, since we can just reference by value
    // but we do not allow mutable versions since that can easilly cause unexpected behaviour
    let base_string = 
        if str.starts_with("&") && &str[1..] == "self"{
            &str[1..]
        } else {
            &str[..]
        };
    
    if  base_string == "self" ||
        FROM_PRIMITIVES.contains(&base_string) ||
        (base_string.starts_with("Lua") &&
        config.types.contains_key(&base_string[WRAPPER_PREFIX.len()..])){
        return true
    };


    false
}

pub(crate) fn is_valid_lua_fn_return_typ(str : &str, config : &Config) -> bool{
    const TO_PRIMITIVES : [&str;22] = ["bool","StdString","&str","Box<str>","CString","&CStr","BString","&BStr","i8","u8","i16","u16","i32","u32","i64","u64","i128","u128","isize","usize","f32","f64"];

    // TODO: support slices of supported types + Cow strings

    // we also allow references to these, since we can just reference by value
    // but we do not allow mutable versions since that can easilly cause unexpected behaviour
    
    if TO_PRIMITIVES.contains(&str) ||
        (str.starts_with("Lua") &&
        config.types.contains_key(&str[WRAPPER_PREFIX.len()..])){
        return true
    };

    false
}

/// standardizes simple function arguments identifiers to auto method format
pub(crate) fn to_auto_method_argument(base_string : &String,wrapped: &WrappedItem, config : &Config, is_first_arg : bool) -> Result<String,String>{
    let underlying_type = 
        if base_string == "Self"{
            if is_first_arg {
                return Ok("self".to_owned())
            } else {
                wrapped.wrapped_type
            }
        } else {
            base_string
        };

    if config.types.contains_key(underlying_type){
        // wrap things that need wrapped
        Ok(format!("{WRAPPER_PREFIX}{underlying_type}"))
    } else if config.primitives.contains(underlying_type) {
        Ok(underlying_type.to_string())
    } else {
        Err(underlying_type.to_owned())
    }
    
}

pub(crate) fn to_op_argument(base_string: &String, self_type : &String, wrapped : &WrappedItem, config : &Config, is_first_arg : bool, is_return_type : bool) -> Result<String,String>{
        // first of all deal with Self arguments
    // return if needs to just be self
    // otherwise get unwrapped type name

    let self_on_lhs = self_type == wrapped.wrapped_type;

    let underlying_type = 
        if base_string == "Self" && self_on_lhs && is_first_arg{
            return Ok("self".to_owned())
        } else if base_string == "Self"{
            &self_type
        } else {
            if !self_on_lhs && !is_return_type{
                return Ok("self".to_owned())
            } else{
                base_string
            }
        };

    if config.types.contains_key(underlying_type){
        // wrap things that need wrapped
        Ok(format!("{WRAPPER_PREFIX}{underlying_type}"))
    } else if config.primitives.contains(underlying_type) {
        Ok(underlying_type.to_string())
    } else {
        Err(underlying_type.to_owned())
    }
}

/// Converts an arbitary type to its simple string representation while converting the base type identifier with the given function
pub(crate) fn type_to_string<F : Fn(&String) -> Result<String,String>>(t : &Type, f : &F) -> Result<String,String> {
    match t {
        Type::ResolvedPath { name, .. } | 
        Type::Generic(name) => // For some reason Self is a generic
                f(name)
        ,
        Type::Primitive(v) => Ok(v.to_string()),
        Type::Tuple(v) => Ok(format!("({})",v.iter().map(|t| type_to_string(t,f.clone())).collect::<Result<Vec<_>,_>>()?.join(","))),
        Type::Slice(v) => Ok(format!("[{}]",type_to_string(v,f)?)),
        Type::Array { type_, len } => Ok(format!("[{};{}]",type_to_string(type_,f)?,len)),
        Type::BorrowedRef { lifetime, mutable, type_ } => {
            
            let base = type_to_string(type_,f)?;
            let inner = format!("&{}{}{}",
                lifetime.as_ref()
                        .map(|v| format!("'{v} "))
                        .unwrap_or_default(),
                mutable.then(|| "mut ")
                    .unwrap_or_default(),
                base
            );
            Ok(inner)
            
        },
        _ => Err(format!("{t:#?}"))
    }
}

impl WrappedItem<'_> {

    fn docstringify(s: String,tabs: usize) -> String {
        s.lines()
        .map(|l| format!("\n{}///{l}","\t".repeat(tabs)))
        .collect()
    }

    pub fn get_full_path(&self) -> String {
        if self.config.import_path.is_empty(){
            self.path_components.join("::")
        } else {
            self.config.import_path.to_owned()
        }
    }

    pub fn get_type_docstring(&self) -> String{
        Self::docstringify(if let Some(d) = &self.config.doc {
            d.to_string()
        } else {
            self.item.docs
            .as_ref()
            .cloned()
            .unwrap_or_else(||"".to_string())
        },1)
    }

    pub fn get_method_docstring(&self, id : &Id) -> String{
        Self::docstringify(self.source.index
                .get(id)
                .unwrap().docs
                .as_ref()
                .cloned()
                .unwrap_or_else(||"".to_owned())
            ,3)
    }

    pub fn get_impl_block_body(&self) -> String {
        self.config.lua_methods
            .iter()
            .map(|v| format!("\n\t\t\t{v};"))
            .collect::<Vec<_>>().join("")
    }

    pub fn get_derive_flags_body(&self, config: &Config) -> String {
        let auto_methods : Option<String> = self.self_impl.map(|v| 
            v.items.iter() 
            .map(|v|
                self.source.index.get(v).unwrap()     
            )
            .filter_map(|v| { 

                let decl = match &v.inner {
                    ItemEnum::Function(f) => &f.decl,
                    ItemEnum::Method(m) => &m.decl,
                    _ => return None,
                    };


                let fn_name = v.name.as_ref().unwrap();
                let args = decl.inputs
                    .iter()
                    .enumerate()
                    .map(|(i,(_,tp))| {
                        let out = type_to_string(tp, &|base_string : &String| to_auto_method_argument(base_string,self,config,i==0))?;
                        if !is_valid_lua_fn_arg(&out, config){
                            return Err(format!("{out}"))
                        }
                        Ok(out)
                    })
                    .collect::<Result<Vec<_>,String>>();

                let return_tp = decl.output
                    .as_ref()
                    .map(|tp| {
                        let out = type_to_string(tp, &|base_string : &String| to_auto_method_argument(base_string,self,config,false))?;
                        if !is_valid_lua_fn_return_typ(&out, config){
                            return Err(format!("{out}"))
                        }
                        Ok(out)
                    });

                let error = 
                    if args.is_err() {
                        let typ = args.as_ref().err().unwrap();
                        Some(format!("Unsupported argument `{typ}` in type: `{:?}`.",self.item.name))
                    } else if return_tp.as_ref().map(|v| v.is_err()).unwrap_or(false){
                        let typ = return_tp.as_ref().unwrap().as_ref().err().unwrap();
                        Some(format!("Unsupported return type `{typ}` in type: `{:?}`.",self.item.name))
                    } else {
                        None
                    };


                let args = args.unwrap_or_default().join(",");
                let return_tp = return_tp.map(|v| format!("-> {}",v.unwrap_or_default()))
                    .unwrap_or("".to_owned());
                
                let docstring = self.get_method_docstring(&v.id);

                let element = format!("\t{docstring}\n\t\t\t{fn_name}({args}) {return_tp} ");

                if error.is_some() {
                    // comment out for clarity on what's missing in the lua version
                    Some(element.lines().map(|l| format!("\n//{l}") ).collect::<String>() + &format!("\n//\t\t\tError: {}",&error.unwrap()))
                } else {
                    Some(element)
                }

            })
            .collect::<Vec<_>>()
            .join(",\n\t\t")
        );
            
        
        static BINARY_OPS : [(&str,&str); 5] = [("add","Add"),
                                        ("sub","Sub"),
                                        ("div","Div"),
                                        ("mul","Mul"),
                                        ("rem","Rem")];

        static UNARY_OPS : [(&str,&str);1] = [("neg","Neg")];

        let unary_ops = UNARY_OPS.into_iter().flat_map(|(op,rep)|{
            self.impl_items.get(op).map(|items|{
                items.iter().map(|(_,_)|{
                    format!("{rep} self")
                }).collect::<Vec<_>>()
            }).unwrap_or_default()
        }).collect::<Vec<_>>()
            .join(",\n\t\t\t");

        let binary_ops = BINARY_OPS.into_iter().flat_map(|(op,rep) |{
            self.impl_items.get(op).map(|items| {
                    items.iter().map(|(impl_,item)| {
                        let self_type = type_to_string(&impl_.for_,&|s : &String| Ok(s.to_string()));
                        if let Ok(self_type) = self_type {

                            if (&self_type == self.wrapped_type && config.types.contains_key(&self_type)) 
                                || config.primitives.contains(&self_type) {
                                return match &item.inner {
                                    ItemEnum::Method(m) => {
                                        m.decl.inputs
                                            .iter()
                                            .enumerate()
                                            .map(|(idx,(_,t))| 
                                                type_to_string(t, &|b: &String| to_op_argument(b, &self_type, self, &config, idx == 0,false))
                                                .ok()
                                                .and_then(|v | (!v.is_empty()).then_some(v))
                                            ).collect::<Option<Vec<_>>>()
                                            .and_then(|v| Some(v.join(&format!(" {} ",rep))))
                                            .and_then(|mut expr| {
                                                // then provide return type
                                                // for these traits that's on associated types within the impl

                                                let out_type = impl_.items.iter().find_map(|v| {
                                                    let item = self.source.index.get(v).unwrap();
                                                    if let ItemEnum::Typedef(t)= &item.inner{
                                                        match item.name.as_ref().map(|v| v.as_str()) {
                                                            Some("Output") => return Some(&t.type_),
                                                            _ => {}
                                                        }
                                                    }
                                                    None
                                                })?;

                                                let return_string = type_to_string(out_type, &|b: &String| to_op_argument(b, &self_type, &self, &config, false,true))
                                                    .ok()?;

                                                expr = format!("{expr} -> {return_string}");

                                                Some(expr)
                                            })
                                            .unwrap_or_else(|| format!("// Error: unsupported type in `{:?}`",m))
                                        
                                    },
                                    _ => panic!("ads")
                                }
                            }

                        } 

                        format!("\n//\t\t\t Error: unsupported lhs operator `{:?}` in `{rep}`",impl_.for_)
                        
                    }).collect::<Vec<_>>()
                        .join(",\n\t\t\t")
            })
        }).filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(",\n\t\t\t");

        let mut additional = self.config.derive_flags.join("\n\t\t+ ");
        (!additional.is_empty()).then(|| additional.extend("\n\t\t+ ".chars()));

        let auto_methods = auto_methods.map(|v| format!(" \n\t\t+ AutoMethods(\n\t\t{v}\n\t\t)")).unwrap_or("".to_owned());

        format!("{additional}UnaryOps(\n\t\t\t{unary_ops}\n\t\t\t) \n\t\t+ BinOps(\n\t\t\t{binary_ops}\n\t\t\t){auto_methods}")
    }


}

pub(crate) fn generate_macros(crates: &[Crate], config: Config) -> Result<String,io::Error> {

    // the items we want to generate macro instantiations for
    let mut wrapped_items : Vec<_> = crates.iter().flat_map(|source| source.index
        .iter()
        .filter(|(_,item)| item.name
                                    .as_ref()
                                    .and_then(|k|  config.types.get(k))
                                    .and_then(|k| Some(k.matches_result(item,source)))
                                    .unwrap_or(false))
        .map(|(id,item)| {
            
            // extract all available associated constants,methods etc available to this item
            
            let mut self_impl : Option<&Impl> = None;
            let mut impl_items: IndexMap<&str,Vec<(&Impl,&Item)>> = Default::default();

            let impls = match &item.inner{
                ItemEnum::Struct(s) => &s.impls,
                ItemEnum::Enum(e) => &e.impls,
                _ => panic!("Only structs or enums are allowed!")
            };


            impls.iter().for_each(|id| 
                if let ItemEnum::Impl(i) = &source.index.get(id).unwrap().inner {
                    if i.trait_.is_none(){
                        self_impl = Some(i);
                    }
                    i.items.iter().for_each(|id| {
                        let it = source.index.get(id).unwrap();

                        impl_items.entry(it.name.as_ref().unwrap().as_str())
                                    .or_default()
                                    .push((i,it));

                    })
                    
                } else {
                    panic!("Expected impl items here!")
                }
            );
            
            
            
            let config = config.types.get(item.name.as_ref().unwrap()).unwrap();

            let wrapper_type = config.wrapper_type;
            
            let path_components = &source.paths.get(id).unwrap().path;

            let wrapper_name = format!("{WRAPPER_PREFIX}{}",item.name.as_ref().unwrap());
            let wrapped_type = item.name.as_ref().unwrap();
            WrappedItem {
                wrapper_type,
                wrapper_name,
                wrapped_type,
                path_components,
                source,
                config,
                item,
                self_impl,
                impl_items,
            }
        }
        )
    )
    .collect();

    // we want to preserve the original ordering from the config file
    wrapped_items.sort_by_cached_key(|f| config.types.get_index_of(f.wrapped_type).unwrap());

    let macro_list_body : String = wrapped_items.iter().map(|v| {


        let full_path = v.get_full_path();
        let type_docstring : String = v.get_type_docstring();
        let wrapper_type = v.wrapper_type.to_string();
        let mut flags = v.get_derive_flags_body(&config);
        let mut lua_impl_block = v.get_impl_block_body();
         
        if !flags.is_empty(){
            flags = format!(":\n        {flags}");
        }
        if !lua_impl_block.is_empty() {
            lua_impl_block = format!("\n\timpl {{\n{lua_impl_block}}}");
        }

        let non_reflect_inner = if let WrapperType::NonReflect = v.wrapper_type{
            format!("({})",v.wrapped_type)
        } else {
            Default::default()
        };

        format!(
"{{
    {type_docstring}
    {full_path} : {wrapper_type}{non_reflect_inner}{flags} {lua_impl_block}
}},\n")
    }).collect();

    let primitives = r#"
    {
            usize : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<usize>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_usize().ok_or_else(||Error::RuntimeError("Value not compatibile with usize".to_owned()))?)));
            }
    },
    {
            isize : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<isize>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_isize().ok_or_else(||Error::RuntimeError("Value not compatibile with isize".to_owned()))?)));
            }
    },
    {
            i128 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<i128>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_i128().ok_or_else(||Error::RuntimeError("Value not compatibile with i128".to_owned()))?)));
            }
    },
    {
            i64 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<i64>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_i64().ok_or_else(||Error::RuntimeError("Value not compatibile with i64".to_owned()))?)));
            }
    },
    {
            i32 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<i32>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_i32().ok_or_else(||Error::RuntimeError("Value not compatibile with i32".to_owned()))?)));
            }
    },
    {
            i16 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<i16>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_i16().ok_or_else(||Error::RuntimeError("Value not compatibile with i16".to_owned()))?)));
            }
    },
    {
            i8 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<i8>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_i8().ok_or_else(||Error::RuntimeError("Value not compatibile with i8".to_owned()))?)));
            }
    },
    {
            u128 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<u128>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_u128().ok_or_else(||Error::RuntimeError("Value not compatibile with u128".to_owned()))?)));
            }
    },
    {
            u64 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<u64>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_u64().ok_or_else(||Error::RuntimeError("Value not compatibile with u64".to_owned()))?)));
            }
    },
    {
            u32 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<u32>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_u32().ok_or_else(||Error::RuntimeError("Value not compatibile with u32".to_owned()))?)));
            }
    },
    {
            u16 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<u16>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_u16().ok_or_else(||Error::RuntimeError("Value not compatibile with u16".to_owned()))?)));
            }
    },
    {
            u8 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Integer(s.downcast_ref::<u8>().unwrap().to_i64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_integer(v)?.ok_or_else(||Error::RuntimeError("Not an integer".to_owned()))?.to_u8().ok_or_else(||Error::RuntimeError("Value not compatibile with u8".to_owned()))?)));
            }
    },
    {
            f32 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Number(s.downcast_ref::<f32>().unwrap().to_f64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_number(v)?.ok_or_else(||Error::RuntimeError("Not a number".to_owned()))?.to_f32().ok_or_else(||Error::RuntimeError("Value not compatibile with f32".to_owned()))?)));
            }
    },
    {
            f64 : Primitive
            impl {
            "to" => |r,_| r.get(|s,_| Value::Number(s.downcast_ref::<f64>().unwrap().to_f64().unwrap()));
            "from" =>   |r,c,v : Value| r.get_mut(|s,_| Ok(s.apply(&c.coerce_number(v)?.ok_or_else(||Error::RuntimeError("Not a number".to_owned()))?.to_f64().ok_or_else(||Error::RuntimeError("Value not compatibile with f64".to_owned()))?)));
            }
    },
    {
            alloc::string::String : Primitive
            impl {
            "to" => |r,c| r.get(|s,_| Value::String(c.create_string(s.downcast_ref::<String>().unwrap()).unwrap()));
            "from" =>   |r,c,v : Value| c.coerce_string(v)?.ok_or_else(||Error::RuntimeError("Not a string".to_owned())).and_then(|string| r.get_mut(|s,_| Ok(s.apply(&string.to_str()?.to_owned()))));                             //      
            }
    },
    "#;

    let imports = &config.imports;
    let external_types = &config.external_types.join(",");

    let full_macro_invocation = format!("{imports}\n impl_lua_newtypes!([{external_types}][\n{primitives}{macro_list_body}]);");

    Ok(full_macro_invocation)
}

pub fn main() -> Result<(),io::Error>{
    let args = Args::parse();



    let crates : Vec<_> = args.json.into_iter().map(|json| {
        let f = File::open(&json).expect(&format!("Could not open {}", &json));
        let rdr = BufReader::new(f);
        from_reader(rdr)
    }).collect::<Result<Vec<_>,_>>()?;


    let f = read_to_string(args.config)?;
    let mut config: Config = toml::from_str(&f)?;

    config.types_.reverse();

    while !config.types_.is_empty(){
        let t = config.types_.remove(config.types_.len() - 1);
        config.types.insert(t.type_.to_string(),t);
    }



    let out = generate_macros(&crates,config)?;

    println!("{}",out);

    Ok(())
}