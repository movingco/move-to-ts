use crate::ast_exp::*;
use crate::ast_tests::check_test;
use crate::shared::*;
use crate::tsgen_writer::TsgenWriter;
use crate::utils::rename;
use itertools::Itertools;
use move_compiler::{
    diagnostics::{Diagnostic, Diagnostics},
    expansion::ast::ModuleIdent,
    hlir::ast::*,
    naming::ast::{BuiltinTypeName_, StructTypeParameter},
    parser::ast::{Ability_, ConstantName, FunctionName, StructName, Var, Visibility},
};
use move_ir_types::location::Loc;
use std::collections::BTreeSet;

pub fn translate_module(
    mident: ModuleIdent,
    mdef: &ModuleDefinition,
    c: &mut Context,
) -> Result<(String, String), Diagnostics> {
    let filename = format!(
        "{}/{}.ts",
        format_address(mident.value.address),
        mident.value.module
    );
    c.reset_for_module(mident);
    let content = to_ts_string(&(mident, mdef), c);
    match content {
        Err(diag) => {
            let mut diags = Diagnostics::new();
            diags.add(diag);
            Err(diags)
        }
        Ok(res) => Ok((filename, res)),
    }
}

pub fn to_ts_string(v: &impl AstTsPrinter, c: &mut Context) -> Result<String, Diagnostic> {
    let mut writer = TsgenWriter::new();
    v.write_ts(&mut writer, c)?;
    let mut lines = vec![
        "import * as $ from \"@manahippo/move-to-ts\";".to_string(),
        "import {AptosDataCache, AptosParserRepo} from \"@manahippo/move-to-ts\";".to_string(),
        "import {U8, U64, U128} from \"@manahippo/move-to-ts\";".to_string(),
        "import {u8, u64, u128} from \"@manahippo/move-to-ts\";".to_string(),
        "import {TypeParamDeclType, FieldDeclType} from \"@manahippo/move-to-ts\";".to_string(),
        "import {AtomicTypeTag, StructTag, TypeTag, VectorTag} from \"@manahippo/move-to-ts\";"
            .to_string(),
        "import {HexString, AptosClient} from \"aptos\";".to_string(),
    ];
    for package_name in c.package_imports.iter() {
        lines.push(format!(
            "import * as {} from \"../{}\";",
            package_name, package_name
        ));
    }
    for module_name in c.same_package_imports.iter() {
        lines.push(format!(
            "import * as {} from \"./{}\";",
            module_name, module_name
        ));
    }
    lines.push(format!("{}", writer));
    Ok(lines.join("\n"))
}

impl AstTsPrinter for (ModuleIdent, &ModuleDefinition) {
    const CTOR_NAME: &'static str = "ModuleDefinition";
    fn write_ts(&self, w: &mut TsgenWriter, c: &mut Context) -> WriteResult {
        /*
        - imports (handled by TsgenWriter)
        - constants
        - structs
        - functions
         */
        let (name, module) = self;
        let ModuleDefinition {
            package_name,
            attributes: _,
            is_source_module: _,
            dependency_order: _,
            friends: _,
            structs,
            constants,
            functions,
        } = module;

        let package_name = package_name.map_or("".to_string(), |symbol| symbol.to_string());

        // module meta
        w.export_const("packageName", quote(&package_name));
        w.export_const(
            "moduleAddress",
            format!(
                "new HexString({})",
                quote(&format_address_hex(name.value.address))
            ),
        );
        w.export_const("moduleName", quote(&name.value.module.0));
        w.new_line();

        // constants
        for (cname, cdef) in constants.key_cloned_iter() {
            (cname, cdef).write_ts(w, c)?;
        }
        w.new_line();

        // structs
        for (sname, sdef) in structs.key_cloned_iter() {
            (sname, sdef).write_ts(w, c)?;
        }

        // functions
        for (fname, fdef) in functions.key_cloned_iter() {
            (fname, fdef).write_ts(w, c)?;
        }

        Ok(())
    }
}

impl AstTsPrinter for ConstantName {
    const CTOR_NAME: &'static str = "_ConstantName";
    fn term(&self, _c: &mut Context) -> TermResult {
        Ok(rename(self))
    }
}

impl AstTsPrinter for FunctionName {
    const CTOR_NAME: &'static str = "_FunctionName";
    fn term(&self, _c: &mut Context) -> TermResult {
        Ok(rename(self))
    }
}

impl AstTsPrinter for StructName {
    const CTOR_NAME: &'static str = "_StructName";
    fn term(&self, _c: &mut Context) -> TermResult {
        Ok(rename(self))
    }
}

pub fn write_simplify_constant_block(
    block: &Block,
    w: &mut TsgenWriter,
    c: &mut Context,
) -> WriteResult {
    if block.len() == 1 {
        match &block[0].value {
            Statement_::Command(cmd) => match &cmd.value {
                Command_::Return { from_user: _, exp } => {
                    w.write(exp.term(c)?);
                    return Ok(());
                }
                _ => (),
            },
            _ => (),
        }
    }
    // write block as lambda
    w.write("( () => ");
    block.write_ts(w, c)?;
    w.write(")()");
    Ok(())
}

impl AstTsPrinter for (ConstantName, &Constant) {
    const CTOR_NAME: &'static str = "ConstantDef";
    fn write_ts(&self, w: &mut TsgenWriter, c: &mut Context) -> WriteResult {
        let (
            name,
            Constant {
                attributes: _,
                loc: _loc,
                signature,
                value,
            },
        ) = self;
        let (_, value_block) = value;
        let typename = ts_constant_type(signature, c)?;
        w.write(format!("export const {} : {} = ", name.term(c)?, typename));
        // FIXME this is a block
        write_simplify_constant_block(value_block, w, c)?;
        w.writeln(";");
        Ok(())
    }
}

impl AstTsPrinter for StructTypeParameter {
    // only used by (StructName, &StructDefinition)
    const CTOR_NAME: &'static str = "StructTypeParameter";
    fn term(&self, _c: &mut Context) -> TermResult {
        let Self { is_phantom, param } = self;
        let name = rename(&quote(&param.user_specified_name));
        Ok(format!("{{ name: {}, isPhantom: {} }}", name, is_phantom))
    }
}

impl AstTsPrinter for (StructName, &StructDefinition) {
    const CTOR_NAME: &'static str = "StructDef";
    fn write_ts(&self, w: &mut TsgenWriter, c: &mut Context) -> WriteResult {
        let (name, sdef) = self;

        w.new_line();
        w.writeln(format!("export class {} ", name.term(c)?));
        w.short_block(|w| {
            w.writeln("static moduleAddress = moduleAddress;");
            w.writeln("static moduleName = moduleName;");
            w.writeln(format!("static structName: string = {};", quote(&name.term(c)?)));

            // 0. type parameters
            // 1. static field decl
            // 2. actual field decl
            // 3. ctor
            // 4. static parser
            // 5. resource loader

            // 0: type parameters
            w.write("static typeParameters: TypeParamDeclType[] = [");
            w.indent(2, |w| {
                w.list(&sdef.type_parameters, ",", |w, struct_tparam| {
                    w.write(struct_tparam.term(c)?);
                    Ok(true)
                })?;
                Ok(())
            })?;
            w.writeln("];");
            match &sdef.fields {
                StructFields::Native(_) => (),
                StructFields::Defined(fields) => {

                    // 1: static field decls
                    w.writeln("static fields: FieldDeclType[] = [");
                    w.list(fields, ",", |w, (name, ty)| {
                        w.write(format!(
                            "{{ name: {}, typeTag: {} }}",
                            quote(&rename(&name)),
                            base_type_to_typetag_builder(ty, &sdef.type_parameters, c)?
                        ));
                        Ok(true)
                    })?;
                    w.writeln("];");
                    w.new_line();

                    // 2. actual class fields
                    if !fields.is_empty() {
                        w.list(fields, "", |w, (name, ty)| {
                            w.write(format!("{}: {};", rename(&name), base_type_to_tstype(ty, c)?));
                            Ok(true)
                        })?;
                        w.new_line();
                        w.new_line();
                    }

                    // 3. ctor
                    w.write("constructor(proto: any, public typeTag: TypeTag) {");
                    w.indent(2, |w| {
                        // one line for each field
                        w.list(fields, "", |w, (name, ty)| {
                            let name = rename(&name);
                            let tstype = base_type_to_tstype(ty, c)?;
                            w.write(
                                format!("this.{} = proto['{}'] as {};", name, name, tstype));
                            Ok(true)
                        })?;
                        Ok(())
                    })?;
                    w.writeln("}");

                    // 4. static Parser
                    w.new_line();
                    w.writeln(format!("static {}Parser(data:any, typeTag: TypeTag, repo: AptosParserRepo) : {} {{", name, name));
                    w.writeln(format!("  const proto = $.parseStructProto(data, typeTag, repo, {});", name));
                    w.writeln(format!("  return new {}(proto, typeTag);", name));
                    w.writeln("}");

                    // 5. resource loader
                    if sdef.abilities.has_ability_(Ability_::Key) {
                        w.new_line();
                        w.writeln("static async load(repo: AptosParserRepo, client: AptosClient, address: HexString, typeParams: TypeTag[]) {");
                        w.writeln(format!("  const result = await repo.loadResource(client, address, {}, typeParams);", name));
                        w.writeln(format!("  return result as unknown as {};", name));
                        w.write("}");
                    }
                }
            };
            Ok(())
        })?;
        w.new_line();

        Ok(())
    }
}

pub fn write_parameters(
    sig: &FunctionSignature,
    w: &mut TsgenWriter,
    c: &mut Context,
) -> WriteResult {
    w.increase_indent();
    for (name, ty) in &sig.parameters {
        w.writeln(format!(
            "{}: {},",
            rename(name),
            single_type_to_tstype(ty, c)?
        ));
    }
    w.decrease_indent();

    Ok(())
}

impl AstTsPrinter for (FunctionName, &Function) {
    const CTOR_NAME: &'static str = "FunctionDef";
    fn write_ts(&self, w: &mut TsgenWriter, c: &mut Context) -> WriteResult {
        let (name, func) = self;
        let is_entry = matches!(func.visibility, Visibility::Script(_));
        if c.config.test {
            let is_test = check_test(name, func, c)?;
            if is_test {
                w.writeln("// test func");
            }
        }
        // yep, regardless of visibility, we always export it
        w.writeln(format!("export function {}$ (", rename(name)));
        // write parameters
        write_parameters(&func.signature, w, c)?;
        // cache & typeTags
        w.writeln("  $c: AptosDataCache,");
        let num_tparams = func.signature.type_parameters.len();
        let tpnames = if num_tparams == 0 {
            "".to_string()
        } else {
            func.signature
                .type_parameters
                .iter()
                .map(|tp| tp.user_specified_name.to_string())
                .join(", ")
        };
        if num_tparams > 0 {
            w.writeln(format!("  $p: TypeTag[], /* <{}>*/", tpnames));
        }
        // marks returnType or void
        w.write("): ");
        let ret_type_str = type_to_tstype(&func.signature.return_type, c)?;
        w.write(ret_type_str);
        w.write(" ");

        // set current_function_signature as we enter body
        c.current_function_signature = Some(func.signature.clone());
        //assert!(c.local_decl_stack.len() == 0);
        //c.push_new_local_frame();
        // add parameters to local frame
        let mut param_names = BTreeSet::new();
        for (name, _) in func.signature.parameters.iter() {
            param_names.insert(name.to_string());
        }
        match &func.body.value {
            FunctionBody_::Native => {
                let mident = c.current_module.unwrap();
                let native_name = format!(
                    "return $.{}_{}_{}",
                    format_address(mident.value.address),
                    mident.value.module,
                    name
                );
                let args = func
                    .signature
                    .parameters
                    .iter()
                    .map(|(n, _)| rename(&n.to_string()))
                    .join(", ");
                let args_comma = format!("{}{}", args, if args.is_empty() { "" } else { ", " });
                let comma_tags = format!(
                    "{}{}",
                    if num_tparams == 0 { "" } else { ", " },
                    if num_tparams == 0 {
                        "".to_string()
                    } else {
                        format!(
                            "[{}]",
                            (0..num_tparams)
                                .into_iter()
                                .map(|u| format!("$p[{}]", u))
                                .join(", ")
                        )
                    }
                );
                w.short_block(|w| {
                    w.writeln(format!("{}({}$c{});", native_name, args_comma, comma_tags));
                    Ok(())
                })?;
            }
            FunctionBody_::Defined { locals, body } => {
                let new_vars = locals
                    .key_cloned_iter()
                    .map(|(name, _)| name)
                    .filter(|name| !param_names.contains(&name.to_string()))
                    .collect::<Vec<_>>();
                write_func_body(body, &new_vars, w, c)?;
            }
        }
        //c.pop_local_frame();
        //assert!(c.local_decl_stack.len() == 0);
        w.new_line();

        if is_entry {
            // TODO
            // uses entry-func signature, which returns TransactionInfo{toPayload(), send(),
            // sendAndWait()}
            w.new_line();
            // yep, regardless of visibility, we always export it
            w.writeln(format!("export function buildPayload_{} (", name));
            // write parameters
            write_parameters(&func.signature, w, c)?;
            // typeTags
            if num_tparams > 0 {
                w.writeln(format!("  $p: TypeTag[], /* <{}>*/", tpnames));
            }
            // marks returnType or void
            w.write(") ");
            // body:
            let params_no_signers = func
                .signature
                .parameters
                .iter()
                .filter(|(_n, ty)| !is_type_signer(ty))
                .collect::<Vec<_>>();

            w.short_block(|w| {
                let mident = c.current_module.unwrap();
                let address = format_address_hex(mident.value.address);
                if num_tparams > 0 {
                    w.writeln("const typeParamStrings = $p.map(t=>$.getTypeTagFullname(t));");
                } else {
                    w.writeln("const typeParamStrings = [] as string[];");
                }
                w.writeln("return $.buildPayload(");
                // function_name
                w.writeln(format!(
                    "  \"{}::{}::{}\",",
                    address, mident.value.module, name
                ));
                // type arguments
                w.writeln("  typeParamStrings,");
                // arguments
                if params_no_signers.is_empty() {
                    w.writeln("  []");
                } else {
                    w.writeln("  [");
                    for (pname, ptype) in params_no_signers.iter() {
                        w.writeln(format!(
                            "    {},",
                            get_ts_handler_for_script_function_param(pname, ptype)?,
                        ));
                    }
                    w.writeln("  ]");
                }
                w.writeln(");");
                Ok(())
            })?;
            w.new_line();
        }

        c.current_function_signature = None;

        Ok(())
    }
}

pub fn extract_builtin_from_base_type(
    ty: &BaseType,
) -> Result<(&BuiltinTypeName_, &Vec<BaseType>), bool> {
    if let BaseType_::Apply(_, typename, ty_args) = &ty.value {
        if let TypeName_::Builtin(builtin) = &typename.value {
            return Ok((&builtin.value, ty_args));
        }
    }
    Err(false)
}

pub fn extract_builtin_type(ty: &SingleType) -> Result<(&BuiltinTypeName_, &Vec<BaseType>), bool> {
    match &ty.value {
        SingleType_::Base(base_ty) => extract_builtin_from_base_type(base_ty),
        SingleType_::Ref(_, base_ty) => extract_builtin_from_base_type(base_ty),
    }
}

pub fn get_ts_handler_for_script_function_param(name: &Var, ty: &SingleType) -> TermResult {
    let name = rename(name);
    if let Ok((builtin, ty_args)) = extract_builtin_type(ty) {
        match builtin {
            BuiltinTypeName_::Bool | BuiltinTypeName_::Address => Ok(name),
            BuiltinTypeName_::U8 | BuiltinTypeName_::U64 | BuiltinTypeName_::U128 => {
                Ok(format!("{}.toPayloadArg()", name))
            }
            BuiltinTypeName_::Signer => unreachable!(),
            BuiltinTypeName_::Vector => {
                // handle vector
                assert!(ty_args.len() == 1);
                if let Ok((inner_builtin, inner_ty_args)) =
                    extract_builtin_from_base_type(&ty_args[0])
                {
                    match inner_builtin {
                        BuiltinTypeName_::Bool | BuiltinTypeName_::Address => Ok(name),
                        BuiltinTypeName_::U8 | BuiltinTypeName_::U64 | BuiltinTypeName_::U128 => {
                            Ok(format!("{}.map(u => u.toPayloadArg())", name))
                        }
                        BuiltinTypeName_::Signer => unreachable!(),
                        BuiltinTypeName_::Vector => {
                            assert!(inner_ty_args.len() == 1);
                            let inner_map = get_ts_handler_for_vector_in_vector(&inner_ty_args[0])?;
                            Ok(format!("{}.map({})", name, inner_map))
                        }
                    }
                } else {
                    derr!((
                        ty.loc,
                        "This vector type is not supported as parameter of a script function"
                    ))
                }
            }
        }
    } else {
        derr!((
            ty.loc,
            "This type is not supported as parameter of script function"
        ))
    }
}

pub fn get_ts_handler_for_vector_in_vector(inner_ty: &BaseType) -> TermResult {
    if let Ok((builtin, inner_ty_args)) = extract_builtin_from_base_type(inner_ty) {
        match builtin {
            BuiltinTypeName_::Bool | BuiltinTypeName_::Address => {
                Ok("array => return array".to_string())
            }
            BuiltinTypeName_::U8 | BuiltinTypeName_::U64 | BuiltinTypeName_::U128 => {
                Ok("array => array.map(u => u.toPayloadArg())".to_string())
            }
            BuiltinTypeName_::Signer => unreachable!(),
            BuiltinTypeName_::Vector => {
                assert!(inner_ty_args.len() == 1);
                let inner_map = get_ts_handler_for_vector_in_vector(&inner_ty_args[0])?;
                Ok(format!("array => array.map({})", inner_map))
            }
        }
    } else {
        derr!((inner_ty.loc, "Unsupported vector-in-vector type"))
    }
}

pub fn is_base_type_signer(ty: &BaseType) -> bool {
    match &ty.value {
        BaseType_::Apply(_, typename, _) => match &typename.value {
            TypeName_::Builtin(builtin) => {
                return builtin.value == BuiltinTypeName_::Signer;
            }
            _ => false,
        },
        _ => false,
    }
}

pub fn is_type_signer(ty: &SingleType) -> bool {
    // includes signer or &signer
    match &ty.value {
        SingleType_::Base(base_ty) => is_base_type_signer(base_ty),
        SingleType_::Ref(_, base_ty) => is_base_type_signer(base_ty),
    }
}

pub fn ts_constant_type(ty: &BaseType, c: &mut Context) -> TermResult {
    // only builtin types allowed as top-level constants
    match &ty.value {
        BaseType_::Apply(_, type_name, type_args) => match &type_name.value {
            TypeName_::Builtin(builtin_type_name) => match builtin_type_name.value {
                BuiltinTypeName_::Vector => {
                    Ok(format!("{}[]", ts_constant_type(&type_args[0], c)?))
                }
                _ => builtin_type_name.term(c),
            },
            _ => unreachable!("Only builtin types supported as constants"),
        },
        _ => unreachable!("Only builtin types supported as constants"),
    }
}

pub fn is_empty_block(block: &Block) -> bool {
    if block.is_empty() {
        return true;
    } else if block.len() == 1 {
        return match &block[0].value {
            Statement_::Command(cmd) => match &cmd.value {
                Command_::IgnoreAndPop { pop_num: _, exp } => is_exp_unit(exp),
                _ => false,
            },
            _ => false,
        };
    }
    false
}

pub fn identify_declared_vars_in_lvalue(lvalue: &LValue, declared: &mut BTreeSet<String>) {
    use LValue_ as L;
    match &lvalue.value {
        L::Ignore => (),
        L::Var(_, _) => {
            //declared.insert(var.to_string());
        }
        L::Unpack(_, _, fields) => {
            for (_, lvalue) in fields.iter() {
                if let LValue_::Var(var, _) = &lvalue.value {
                    declared.insert(var.to_string());
                } else {
                    identify_declared_vars_in_lvalue(lvalue, declared);
                }
            }
        }
    }
}

pub fn identify_declared_vars_in_cmd(cmd: &Command, declared: &mut BTreeSet<String>) {
    use Command_ as C;
    match &cmd.value {
        C::Assign(lvalues, _) => lvalues.iter().for_each(|lvalue| {
            identify_declared_vars_in_lvalue(lvalue, declared);
        }),
        _ => (),
    }
}

pub fn identify_declared_vars_in_stmt(stmt: &Statement, declared: &mut BTreeSet<String>) {
    use Statement_ as S;
    match &stmt.value {
        S::Command(cmd) => identify_declared_vars_in_cmd(cmd, declared),
        S::IfElse {
            cond: _,
            if_block,
            else_block,
        } => {
            identify_declared_vars_in_block(if_block, declared);
            identify_declared_vars_in_block(else_block, declared);
        }
        S::While { cond, block } => {
            let (pre_block, _cond_exp) = cond;
            identify_declared_vars_in_block(block, declared);
            identify_declared_vars_in_block(pre_block, declared);
        }
        S::Loop {
            has_break: _,
            block,
        } => identify_declared_vars_in_block(block, declared),
    };
}

pub fn identify_declared_vars_in_block(block: &Block, undeclared: &mut BTreeSet<String>) {
    for stmt in block.iter() {
        identify_declared_vars_in_stmt(stmt, undeclared);
    }
}

pub fn write_func_body(
    block: &Block,
    new_vars: &Vec<Var>,
    w: &mut TsgenWriter,
    c: &mut Context,
) -> WriteResult {
    w.writeln("{");
    w.increase_indent();

    let mut declared_vars = BTreeSet::<String>::new();
    identify_declared_vars_in_block(block, &mut declared_vars);

    let undeclared = new_vars
        .iter()
        .filter(|var| !declared_vars.contains(&var.to_string()))
        .collect::<Vec<_>>();

    if undeclared.len() > 0 {
        w.writeln(format!(
            "let {};",
            undeclared
                .into_iter()
                .map(|v| rename(&v.to_string()))
                .join(", ")
        ));
        /*
        for v in new_vars.iter() {
            c.declare_local(v.to_string());
        }
         */
    }

    for stmt in block.iter() {
        stmt.write_ts(w, c)?;
    }

    w.decrease_indent();
    w.writeln("}");

    Ok(())
}

impl AstTsPrinter for Block {
    const CTOR_NAME: &'static str = "Block";
    fn write_ts(&self, w: &mut TsgenWriter, c: &mut Context) -> WriteResult {
        w.writeln("{");
        w.increase_indent();

        //c.push_new_local_frame();
        for stmt in self.iter() {
            stmt.write_ts(w, c)?;
        }
        //c.pop_local_frame();

        w.decrease_indent();
        w.writeln("}");

        Ok(())
    }
}

impl AstTsPrinter for Statement {
    const CTOR_NAME: &'static str = "Statement";
    fn write_ts(&self, w: &mut TsgenWriter, c: &mut Context) -> WriteResult {
        use Statement_ as S;
        // some value-yielding Block can be formatted as lambdas, and need statements to be
        // presented in the form of ts_term
        match &self.value {
            S::Command(cmd) => cmd.write_ts(w, c),
            S::IfElse {
                cond,
                if_block,
                else_block,
            } => {
                // FIXME in case it's a single statement, need indentation here
                if !is_empty_block(if_block) {
                    // if-block is non-empty
                    w.write(format!("if ({}) ", cond.term(c)?));
                    if_block.write_ts(w, c)?;
                    if else_block.len() > 0 {
                        w.write("else");
                        else_block.write_ts(w, c)?;
                    }
                } else {
                    // if-block is empty, negate condition and output else block only
                    w.write(format!("if (!{}) ", cond.term(c)?));
                    else_block.write_ts(w, c)?;
                }
                Ok(())
            }
            S::While { cond, block } => {
                let (pre_block, cond_exp) = cond;
                // FIXME need to handle the empty case
                let has_pre_block = pre_block.len() > 0;
                w.write(format!(
                    "while ({}) ",
                    if has_pre_block {
                        "true".to_string()
                    } else {
                        cond_exp.term(c)?
                    }
                ));
                w.short_block(|w| {
                    if has_pre_block {
                        pre_block.write_ts(w, c)?;
                        w.writeln(format!("if (!({})) break;", cond_exp.term(c)?));
                    }
                    block.write_ts(w, c)?;
                    Ok(())
                })?;
                Ok(())
            }
            S::Loop {
                has_break: _,
                block,
            } => {
                w.write("while (true) ");
                block.write_ts(w, c)
            }
        }
    }
}

/*
// returns true if this is a new variable to be declared
fn lvalues_has_new_decl(lvalues: &Vec<LValue>, c: &mut Context) -> Result<bool, Diagnostic> {
    let mut has_new = false;
    for lvalue in lvalues.iter() {
        if lvalue.value == LValue_::Ignore {
            continue;
        }
        let is_new = is_lvalue_new_decl(lvalue, c)?;
        if has_new && !is_new {
            return derr!((
                lvalue.loc,
                "Cannot redeclare existing variables with new variables"
            ));
        }
        has_new = has_new | is_new;
    }
    Ok(has_new)
}
 */

pub fn is_exp_unit(exp: &Exp) -> bool {
    matches!(exp.exp.value, UnannotatedExp_::Unit { case: _})
}

impl AstTsPrinter for Command {
    const CTOR_NAME: &'static str = "Command";

    fn write_ts(&self, w: &mut TsgenWriter, c: &mut Context) -> WriteResult {
        use Command_ as C;
        match &self.value {
            C::Assign(lvalues, rhs) => {
                if is_empty_lvalue_list(lvalues) {
                    w.writeln(format!("{};", rhs.term(c)?));
                } else {
                    /*
                    if lvalues_has_new_decl(lvalues, c)? {
                        w.write("let ");
                    }
                     */
                    if lvalues.len() == 1 && matches!(lvalues[0].value, LValue_::Unpack(_, _, _)) {
                        w.write("let ");
                    }
                    /*
                    if *is_new_decl {
                        w.write("let ");
                    }
                     */
                    // using write_ts instead of term to allow prettier printing in case we ever
                    // want to do that
                    lvalues.write_ts(w, c)?;
                    w.write(" = ");
                    w.write(rhs.term(c)?);
                    w.writeln(";");
                }
            }
            C::Mutate(lhs, rhs) => match &lhs.exp.value {
                UnannotatedExp_::Borrow(_, _, _) => {
                    w.writeln(format!("{} = {};", lhs.term(c)?, rhs.term(c)?));
                }
                _ => {
                    w.writeln(format!("{}.$set({});", lhs.term(c)?, rhs.term(c)?));
                }
            },
            C::Abort(e) => w.writeln(format!("throw {};", e.term(c)?)),
            C::Return { from_user: _, exp } => {
                if is_exp_unit(exp) {
                    w.writeln("return;");
                } else {
                    w.writeln(format!("return {};", exp.term(c)?));
                }
            }
            C::Break => w.writeln("break;"),
            C::Continue => w.writeln("continue;"),
            C::IgnoreAndPop { pop_num: _, exp } => {
                if is_exp_unit(exp) {
                    // do nothing..
                    // w.writeln("/*PopAndIgnore*/");
                } else {
                    w.writeln(format!("{};", exp.term(c)?));
                }
            }
            _ => {
                return derr!((self.loc, "Unsupported Command (Jump)"));
            }
        }
        Ok(())
    }
}