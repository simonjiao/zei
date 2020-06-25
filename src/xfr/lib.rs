use crate::api::anon_creds::ACCommitment;
use crate::errors::ZeiError;
use crate::setup::PublicParams;
use crate::utils::{u64_to_u32_pair, u8_bigendian_slice_to_u128};
use crate::xfr::asset_mixer::{
  batch_verify_asset_mixing, prove_asset_mixing, AssetMixProof, AssetMixingInstance,
};
use crate::xfr::proofs::{
  asset_amount_tracking_proofs, asset_proof, batch_verify_confidential_amount,
  batch_verify_confidential_asset, batch_verify_tracer_tracking_proof, range_proof,
};
use crate::xfr::sig::{sign_multisig, verify_multisig, XfrKeyPair, XfrMultiSig, XfrPublicKey};
use crate::xfr::structs::*;
use bulletproofs::PedersenGens;
use curve25519_dalek::ristretto::CompressedRistretto;
use curve25519_dalek::scalar::Scalar;
use itertools::Itertools;
use rand_core::{CryptoRng, RngCore};
use serde::ser::Serialize;
use std::collections::HashMap;

const POW_2_32: u64 = 0xFFFF_FFFFu64 + 1;

#[derive(Clone, Copy, Debug)]
#[allow(non_camel_case_types)]
#[allow(clippy::enum_variant_names)]
pub(super) enum XfrType {
  /// All inputs and outputs are revealed and all have the same asset type
  NonConfidential_SingleAsset,
  /// At least one input or output has a confidential amount and all asset types are revealed
  ConfidentialAmount_NonConfidentialAssetType_SingleAsset,
  /// At least one asset type is confidential and all the amounts are revealed
  NonConfidentialAmount_ConfidentialAssetType_SingleAsset,
  /// At least one input or output has both confidential amount and asset type
  Confidential_SingleAsset,
  /// At least one input or output has confidential amount and asset type and involves multiple asset types
  Confidential_MultiAsset,
  /// All inputs and outputs reveal amounts and asset types
  NonConfidential_MultiAsset,
}

impl XfrType {
  pub(super) fn from_inputs_outputs(inputs_record: &[AssetRecord],
                                    outputs_record: &[AssetRecord])
                                    -> Self {
    let mut multi_asset = false;
    let mut confidential_amount_nonconfidential_asset_type = false;
    let mut confidential_asset_type_nonconfidential_amount = false;
    let mut confidential_all = false;

    let asset_type = inputs_record[0].open_asset_record.asset_type;
    for record in inputs_record.iter().chain(outputs_record) {
      if asset_type != record.open_asset_record.asset_type {
        multi_asset = true;
      }
      let confidential_amount;
      match record.open_asset_record.blind_asset_record.amount {
        XfrAmount::Confidential(_) => {
          confidential_amount = true;
        }
        _ => {
          confidential_amount = false;
        }
      }
      let confidential_asset_type;
      match record.open_asset_record.blind_asset_record.asset_type {
        XfrAssetType::Confidential(_) => {
          confidential_asset_type = true;
        }
        _ => {
          confidential_asset_type = false;
        }
      }
      if confidential_amount && confidential_asset_type {
        confidential_all = true;
      } else if confidential_amount {
        confidential_amount_nonconfidential_asset_type = true;
      } else if confidential_asset_type {
        confidential_asset_type_nonconfidential_amount = true;
      }
    }
    if multi_asset {
      if confidential_all
         || confidential_amount_nonconfidential_asset_type
         || confidential_asset_type_nonconfidential_amount
      {
        return XfrType::Confidential_MultiAsset;
      } else {
        return XfrType::NonConfidential_MultiAsset;
      }
    }
    if confidential_all
       || (confidential_amount_nonconfidential_asset_type
           && confidential_asset_type_nonconfidential_amount)
    {
      XfrType::Confidential_SingleAsset
    } else if confidential_amount_nonconfidential_asset_type {
      XfrType::ConfidentialAmount_NonConfidentialAssetType_SingleAsset
    } else if confidential_asset_type_nonconfidential_amount {
      XfrType::NonConfidentialAmount_ConfidentialAssetType_SingleAsset
    } else {
      XfrType::NonConfidential_SingleAsset
    }
  }
}

/// I Create a XfrNote from list of opened asset records inputs and asset record outputs
/// * `prng` - pseudo-random number generator
/// * `inputs` - asset records containing amounts, assets, policies and memos
/// * `outputs` - asset records containing amounts, assets, policies and memos
/// * `input_keys`- keys needed to sign the inputs
/// * `returns` an error or an XfrNote
/// # Example
/// ```
/// use rand_chacha::ChaChaRng;
/// use rand_core::SeedableRng;
/// use zei::xfr::sig::XfrKeyPair;
/// use zei::xfr::structs::{AssetRecordTemplate, AssetRecord};
/// use zei::xfr::asset_record::AssetRecordType;
/// use zei::xfr::lib::{gen_xfr_note, verify_xfr_note};
/// use itertools::Itertools;
/// use zei::setup::PublicParams;
///
/// let mut prng = ChaChaRng::from_seed([0u8; 32]);
/// let mut params = PublicParams::new();
/// let asset_type = [0u8; 16];
/// let inputs_amounts = [(10u64, asset_type),
///                       (10u64, asset_type),
///                       (10u64, asset_type)];
/// let outputs_amounts = [(1u64, asset_type),
///                     (2u64, asset_type),
///                     (3u64, asset_type),
///                      (24u64, asset_type)];
///
/// let mut inputs = vec![];
/// let mut outputs = vec![];
///
/// let mut inkeys = vec![];
/// let mut in_asset_records = vec![];
///
/// let asset_record_type = AssetRecordType::NonConfidentialAmount_NonConfidentialAssetType;
///
/// for x in inputs_amounts.iter() {
///   let keypair = XfrKeyPair::generate(&mut prng);
///   let asset_record = AssetRecordTemplate::with_no_asset_tracking( x.0,
///                                        x.1,
///                                        asset_record_type,
///                                        keypair.get_pk_ref().clone());
///
///   inputs.push(AssetRecord::from_template_no_identity_tracking(&mut prng, &asset_record).unwrap());
///
///   in_asset_records.push(asset_record);
///   inkeys.push(keypair);
/// }
///
/// for x in outputs_amounts.iter() {
///     let keypair = XfrKeyPair::generate(&mut prng);
///
///     let ar = AssetRecordTemplate::with_no_asset_tracking(x.0, x.1, asset_record_type, keypair.get_pk_ref().clone());
///     let output = AssetRecord::from_template_no_identity_tracking(&mut prng, &ar).unwrap();
///     outputs.push(output);
/// }
///
/// let xfr_note = gen_xfr_note( &mut prng,
///                              inputs.as_slice(),
///                              outputs.as_slice(),
///                              inkeys.iter().map(|x| x).collect_vec().as_slice()
///                ).unwrap();
/// assert_eq!(verify_xfr_note(&mut prng, &mut params, &xfr_note, &Default::default()), Ok(()));
/// ```

pub fn gen_xfr_note<R: CryptoRng + RngCore>(prng: &mut R,
                                            inputs: &[AssetRecord],
                                            outputs: &[AssetRecord],
                                            input_key_pairs: &[&XfrKeyPair])
                                            -> Result<XfrNote, ZeiError> {
  if inputs.is_empty() {
    return Err(ZeiError::ParameterError);
  }

  check_keys(inputs, input_key_pairs)?;

  let body = gen_xfr_body(prng, inputs, outputs)?;

  let multisig = compute_transfer_multisig(&body, input_key_pairs)?;

  Ok(XfrNote { body, multisig })
}

/// I create the body of a xfr note. This body contains the data to be signed.
/// * `prng` - pseudo-random number generator
/// * `inputs` - asset records containing amounts, assets, policies and memos
/// * `outputs` - asset records containing amounts, assets, policies and memos
/// * `returns` - an XfrBody struct or an error
/// # Example
/// ```
/// use rand_chacha::ChaChaRng;
/// use rand_core::SeedableRng;
/// use zei::xfr::sig::XfrKeyPair;
/// use zei::xfr::structs::{AssetRecordTemplate, AssetRecord};
/// use zei::xfr::asset_record::AssetRecordType;
/// use zei::xfr::lib::{gen_xfr_body,verify_xfr_body};
/// use zei::setup::PublicParams;
///
/// let mut prng = ChaChaRng::from_seed([0u8; 32]);
/// let mut params = PublicParams::new();
/// let asset_type = [0u8; 16];
/// let inputs_amounts = [(10u64, asset_type),
///                       (10u64, asset_type),
///                       (10u64, asset_type)];
/// let outputs_amounts = [(1u64, asset_type),
///                     (2u64, asset_type),
///                     (3u64, asset_type),
///                      (24u64, asset_type)];
///
/// let mut inputs = vec![];
/// let mut outputs = vec![];
///
/// let asset_record_type = AssetRecordType::NonConfidentialAmount_NonConfidentialAssetType;
///
/// for x in inputs_amounts.iter() {
///   let keypair = XfrKeyPair::generate(&mut prng);
///   let ar = AssetRecordTemplate::with_no_asset_tracking( x.0,
///                                        x.1,
///                                        asset_record_type,
///                                        keypair.get_pk_ref().clone(),
///                                        );
///
///   inputs.push(AssetRecord::from_template_no_identity_tracking(&mut prng, &ar).unwrap());
/// }
/// for x in outputs_amounts.iter() {
///     let keypair = XfrKeyPair::generate(&mut prng);
///
///     let ar = AssetRecordTemplate::with_no_asset_tracking(x.0, x.1, asset_record_type, keypair.get_pk());
///     outputs.push(AssetRecord::from_template_no_identity_tracking(&mut prng, &ar).unwrap());
/// }
/// let body = gen_xfr_body(&mut prng, &inputs, &outputs).unwrap();
/// assert_eq!(verify_xfr_body(&mut prng, &mut params, &body, &Default::default()), Ok(()));
/// ```
pub fn gen_xfr_body<R: CryptoRng + RngCore>(prng: &mut R,
                                            inputs: &[AssetRecord],
                                            outputs: &[AssetRecord])
                                            -> Result<XfrBody, ZeiError> {
  if inputs.is_empty() {
    return Err(ZeiError::ParameterError);
  }
  let xfr_type = XfrType::from_inputs_outputs(inputs, outputs);
  check_asset_amount(inputs, outputs)?;

  let single_asset = match xfr_type {
    XfrType::NonConfidential_MultiAsset | XfrType::Confidential_MultiAsset => false,
    _ => true,
  };

  let open_inputs = inputs.iter()
                          .map(|input| &input.open_asset_record)
                          .collect_vec();
  let open_outputs = outputs.iter()
                            .map(|output| &output.open_asset_record)
                            .collect_vec();
  let asset_amount_proof = if single_asset {
    gen_xfr_proofs_single_asset(prng,
                                open_inputs.as_slice(),
                                open_outputs.as_slice(),
                                xfr_type)?
  } else {
    gen_xfr_proofs_multi_asset(open_inputs.as_slice(), open_outputs.as_slice(), xfr_type)?
  };

  //do tracking proofs
  // TODO avoid clones below
  let asset_type_amount_tracking_proof = asset_amount_tracking_proofs(prng, inputs, outputs)?;
  let asset_tracking_proof =
    AssetTrackingProofs { asset_type_and_amount_proofs: asset_type_amount_tracking_proof,
                          inputs_identity_proofs: inputs.iter()
                                                        .map(|input| input.identity_proofs.clone())
                                                        .collect_vec(),
                          outputs_identity_proofs:
                            outputs.iter()
                                   .map(|output| output.identity_proofs.clone())
                                   .collect_vec() };

  let proofs = XfrProofs { asset_type_and_amount_proof: asset_amount_proof,
                           asset_tracking_proof };

  let mut xfr_inputs = vec![];
  for x in open_inputs {
    xfr_inputs.push(x.blind_asset_record.clone())
  }

  let mut xfr_outputs = vec![];
  for x in open_outputs {
    xfr_outputs.push(x.blind_asset_record.clone())
  }

  let tracer_memos = inputs.iter()
                           .chain(outputs)
                           .map(|record_input| {
                             record_input.asset_tracers_memos.clone() // Can I avoid this clone?
                           })
                           .collect_vec();
  let owner_memos = outputs.iter()
                           .map(|record_input| {
                             record_input.owner_memo.clone() // Can I avoid this clone?
                           })
                           .collect_vec();
  Ok(XfrBody { inputs: xfr_inputs,
               outputs: xfr_outputs,
               proofs,
               asset_tracing_memos: tracer_memos,
               owners_memos: owner_memos })
}

fn check_keys(inputs: &[AssetRecord], input_key_pairs: &[&XfrKeyPair]) -> Result<(), ZeiError> {
  if inputs.len() != input_key_pairs.len() {
    return Err(ZeiError::ParameterError);
  }
  for (input, key) in inputs.iter().zip(input_key_pairs.iter()) {
    let inkey = &input.open_asset_record.blind_asset_record.public_key;
    if inkey != key.get_pk_ref() {
      return Err(ZeiError::ParameterError);
    }
  }
  Ok(())
}

fn gen_xfr_proofs_multi_asset(inputs: &[&OpenAssetRecord],
                              outputs: &[&OpenAssetRecord],
                              xfr_type: XfrType)
                              -> Result<AssetTypeAndAmountProof, ZeiError> {
  let pow2_32 = Scalar::from(POW_2_32);

  let mut ins = vec![];

  for x in inputs.iter() {
    let type_as_u128 = u8_bigendian_slice_to_u128(&x.asset_type[..]);
    let type_scalar = Scalar::from(type_as_u128);
    ins.push((x.amount,
              type_scalar,
              x.amount_blinds.0 + pow2_32 * x.amount_blinds.1,
              x.type_blind));
  }

  let mut out = vec![];
  for x in outputs.iter() {
    let type_as_u128 = u8_bigendian_slice_to_u128(&x.asset_type[..]);
    let type_scalar = Scalar::from(type_as_u128);
    out.push((x.amount,
              type_scalar,
              x.amount_blinds.0 + pow2_32 * x.amount_blinds.1,
              x.type_blind));
  }

  match xfr_type {
    XfrType::Confidential_MultiAsset => {
      let mix_proof = prove_asset_mixing(ins.as_slice(), out.as_slice())?;
      Ok(AssetTypeAndAmountProof::AssetMix(mix_proof))
    }
    XfrType::NonConfidential_MultiAsset => Ok(AssetTypeAndAmountProof::NoProof),
    _ => Err(ZeiError::XfrCreationAssetAmountError),
  }
}

fn gen_xfr_proofs_single_asset<R: CryptoRng + RngCore>(
  prng: &mut R,
  inputs: &[&OpenAssetRecord],
  outputs: &[&OpenAssetRecord],
  xfr_type: XfrType)
  -> Result<AssetTypeAndAmountProof, ZeiError> {
  let pc_gens = PedersenGens::default();

  match xfr_type {
    XfrType::NonConfidential_SingleAsset => Ok(AssetTypeAndAmountProof::NoProof),
    XfrType::ConfidentialAmount_NonConfidentialAssetType_SingleAsset => {
      Ok(AssetTypeAndAmountProof::ConfAmount(range_proof(inputs, outputs)?))
    }
    XfrType::NonConfidentialAmount_ConfidentialAssetType_SingleAsset => {
      Ok(AssetTypeAndAmountProof::ConfAsset(asset_proof(prng, &pc_gens, inputs, outputs)?))
    }
    XfrType::Confidential_SingleAsset => {
      Ok(AssetTypeAndAmountProof::ConfAll((range_proof(inputs, outputs)?,
                                           asset_proof(prng, &pc_gens, inputs, outputs)?)))
    }
    _ => Err(ZeiError::XfrCreationAssetAmountError), // Type cannot be multi asset
  }
}

/// Check that for each asset type total input amount >= total output amount,
/// returns Err(ZeiError::XfrCreationAssetAmountError) otherwise.
/// Return Ok(true) if all inputs and outputs involve a single asset type. If multiple assets
/// are detected, then return Ok(false)
fn check_asset_amount(inputs: &[AssetRecord], outputs: &[AssetRecord]) -> Result<(), ZeiError> {
  let mut amounts = HashMap::new();

  for record in inputs.iter() {
    match amounts.get_mut(&record.open_asset_record.asset_type) {
      None => {
        amounts.insert(record.open_asset_record.asset_type,
                       vec![i128::from(record.open_asset_record.amount)]);
      }
      Some(vec) => {
        vec.push(i128::from(record.open_asset_record.amount));
      }
    };
  }

  for record in outputs.iter() {
    match amounts.get_mut(&record.open_asset_record.asset_type) {
      None => {
        amounts.insert(record.open_asset_record.asset_type,
                       vec![-i128::from(record.open_asset_record.amount)]);
      }
      Some(vec) => {
        vec.push(-i128::from(record.open_asset_record.amount));
      }
    };
  }

  for (_, a) in amounts.iter() {
    let sum = a.iter().sum::<i128>();
    if sum < 0i128 {
      return Err(ZeiError::XfrCreationAssetAmountError);
    }
  }

  Ok(())
}

/// I compute a multisignature over the transfer's body
pub(crate) fn compute_transfer_multisig(body: &XfrBody,
                                        keys: &[&XfrKeyPair])
                                        -> Result<XfrMultiSig, ZeiError> {
  let mut vec = vec![];
  body.serialize(&mut rmp_serde::Serializer::new(&mut vec))?;
  Ok(sign_multisig(keys, vec.as_slice()))
}

/// I verify the transfer multisignature over the its body
pub(crate) fn verify_transfer_multisig(xfr_note: &XfrNote) -> Result<(), ZeiError> {
  let mut vec = vec![];
  xfr_note.body
          .serialize(&mut rmp_serde::Serializer::new(&mut vec))?;
  let mut public_keys = vec![];
  for x in xfr_note.body.inputs.iter() {
    public_keys.push(x.public_key)
  }
  verify_multisig(public_keys.as_slice(), vec.as_slice(), &xfr_note.multisig)
}

/// XfrNote verification
/// * `prng` - pseudo-random number generator
/// * `xfr_note` - XfrNote struct to be verified
/// * `policies` - list of set of policies and associated information corresponding to each xfr_note-
/// * `returns` - () or an ZeiError in case of verification error
pub fn verify_xfr_note<R: CryptoRng + RngCore>(prng: &mut R,
                                               params: &mut PublicParams,
                                               xfr_note: &XfrNote,
                                               policies: &XfrNotePolicies)
                                               -> Result<(), ZeiError> {
  batch_verify_xfr_notes(prng, params, &[&xfr_note], &[&policies])
}

/// XfrNote Batch verification
/// * `prng` - pseudo-random number generator
/// * `xfr_notes` - XfrNote structs to be verified
/// * `policies` - list of set of policies and associated information corresponding to each xfr_note
/// * `returns` - () or an ZeiError in case of verification error
pub fn batch_verify_xfr_notes<R: CryptoRng + RngCore>(prng: &mut R,
                                                      params: &mut PublicParams,
                                                      notes: &[&XfrNote],
                                                      policies: &[&XfrNotePolicies])
                                                      -> Result<(), ZeiError> {
  // 1. verify signature
  for xfr_note in notes {
    verify_transfer_multisig(xfr_note)?;
  }

  let bodies = notes.iter().map(|note| &note.body).collect_vec();
  batch_verify_xfr_bodies(prng, params, &bodies, policies)
}

pub(crate) fn batch_verify_xfr_body_asset_records<R: CryptoRng + RngCore>(
  prng: &mut R,
  params: &mut PublicParams,
  bodies: &[&XfrBody])
  -> Result<(), ZeiError> {
  let mut conf_amount_records = vec![];
  let mut conf_asset_type_records = vec![];
  let mut conf_asset_mix_bodies = vec![];

  for body in bodies {
    match &body.proofs.asset_type_and_amount_proof {
      AssetTypeAndAmountProof::ConfAll((range_proof, asset_proof)) => {
        conf_amount_records.push((&body.inputs, &body.outputs, range_proof)); // save for batching
        conf_asset_type_records.push((&body.inputs, &body.outputs, asset_proof));
        // save for batching
      }
      AssetTypeAndAmountProof::ConfAmount(range_proof) => {
        conf_amount_records.push((&body.inputs, &body.outputs, range_proof)); // save for batching
        verify_plain_asset(body.inputs.as_slice(), body.outputs.as_slice())?; // no batching
      }
      AssetTypeAndAmountProof::ConfAsset(asset_proof) => {
        verify_plain_amounts(body.inputs.as_slice(), body.outputs.as_slice())?; // no batching
        conf_asset_type_records.push((&body.inputs, &body.outputs, asset_proof));
        // save for batch proof
      }
      AssetTypeAndAmountProof::NoProof => {
        verify_plain_asset_mix(body.inputs.as_slice(), body.outputs.as_slice())?;
        // no batching
      }
      AssetTypeAndAmountProof::AssetMix(asset_mix_proof) => {
        conf_asset_mix_bodies.push((body.inputs.as_slice(),
                                    body.outputs.as_slice(),
                                    asset_mix_proof));
        // save for batch proof
      }
    }
  }

  // 1. verify confidential amounts
  batch_verify_confidential_amount(prng, params, conf_amount_records.as_slice())?;

  // 2. verify confidential asset_types
  batch_verify_confidential_asset(prng, &params.pc_gens, &conf_asset_type_records)?;

  // 3. verify confidential asset mix proofs
  batch_verify_asset_mix(prng, params, conf_asset_mix_bodies.as_slice())
}

#[derive(Default, Clone)]
pub struct XfrNotePolicies<'b> {
  pub(crate) inputs_tracking_policies: Vec<&'b AssetTracingPolicies>,
  pub(crate) inputs_sig_commitments: Vec<Option<&'b ACCommitment>>,
  pub(crate) outputs_tracking_policies: Vec<&'b AssetTracingPolicies>,
  pub(crate) outputs_sig_commitments: Vec<Option<&'b ACCommitment>>,
}

impl<'b> XfrNotePolicies<'b> {
  pub fn new(inputs_tracking_policies: Vec<&'b AssetTracingPolicies>,
             inputs_sig_commitments: Vec<Option<&'b ACCommitment>>,
             outputs_tracking_policies: Vec<&'b AssetTracingPolicies>,
             outputs_sig_commitments: Vec<Option<&'b ACCommitment>>)
             -> XfrNotePolicies<'b> {
    XfrNotePolicies { inputs_tracking_policies,
                      inputs_sig_commitments,
                      outputs_tracking_policies,
                      outputs_sig_commitments }
  }
}

pub(crate) fn if_some_closure(x: &Option<ACCommitment>) -> Option<&ACCommitment> {
  if (*x).is_some() {
    Some(x.as_ref().unwrap())
  } else {
    None
  }
}

impl<'a> XfrNotePolicies<'a> {
  pub fn from_policies_no_ref(p: &'a XfrNotePoliciesNoRef) -> XfrNotePolicies<'a> {
    XfrNotePolicies::new(p.inputs_tracking_policies.iter().map(|x| x).collect_vec(),
                         p.inputs_sig_commitments
                          .iter()
                          .map(|x| if_some_closure(x))
                          .collect_vec(),
                         p.outputs_tracking_policies.iter().map(|x| x).collect_vec(),
                         p.outputs_sig_commitments
                          .iter()
                          .map(|x| if_some_closure(x))
                          .collect_vec())
  }
}

#[derive(Default, Clone, Serialize, Deserialize, Eq, PartialEq, Debug)]
pub struct XfrNotePoliciesNoRef {
  pub inputs_tracking_policies: Vec<AssetTracingPolicies>,
  pub inputs_sig_commitments: Vec<Option<ACCommitment>>,
  pub outputs_tracking_policies: Vec<AssetTracingPolicies>,
  pub outputs_sig_commitments: Vec<Option<ACCommitment>>,
}

impl XfrNotePoliciesNoRef {
  pub fn new(inputs_tracking_policies: Vec<AssetTracingPolicies>,
             inputs_sig_commitments: Vec<Option<ACCommitment>>,
             outputs_tracking_policies: Vec<AssetTracingPolicies>,
             outputs_sig_commitments: Vec<Option<ACCommitment>>)
             -> XfrNotePoliciesNoRef {
    XfrNotePoliciesNoRef { inputs_tracking_policies,
                           inputs_sig_commitments,
                           outputs_tracking_policies,
                           outputs_sig_commitments }
  }
}

/// XfrBody verification with tracking policies
/// * `prng` - pseudo-random number generator. Needed for verifying proofs in batch.
/// * `body` - XfrBody structure to be verified
/// * `policies` - list of set of policies and associated information corresponding to each xfr_note
/// * `returns` - () or an ZeiError in case of verification error
pub fn verify_xfr_body<R: CryptoRng + RngCore>(prng: &mut R,
                                               params: &mut PublicParams,
                                               body: &XfrBody,
                                               policies: &XfrNotePolicies)
                                               -> Result<(), ZeiError> {
  batch_verify_xfr_bodies(prng, params, &[body], &[policies])
}

/// XfrBodys batch verification
/// * `prng` - pseudo-random number generator. Needed for verifying proofs in batch.
/// * `bodies` - XfrBody structures to be verified
/// * `policies` - list of set of policies and associated information corresponding to each xfr_note
/// * `returns` - () or an ZeiError in case of verification error
pub fn batch_verify_xfr_bodies<R: CryptoRng + RngCore>(prng: &mut R,
                                                       params: &mut PublicParams,
                                                       bodies: &[&XfrBody],
                                                       policies: &[&XfrNotePolicies])
                                                       -> Result<(), ZeiError> {
  // 1. verify amounts and asset types
  batch_verify_xfr_body_asset_records(prng, params, bodies)?;

  // 2. verify tracing proofs
  batch_verify_tracer_tracking_proof(prng, &params.pc_gens, bodies, policies)
}

/// Takes a vector of u64, converts each element to u128 and compute the sum of the new elements.
/// The goal is to avoid integer overflow when adding several u64 elements together.
fn safe_sum_u64(terms: &[u64]) -> u128 {
  terms.iter().map(|x| u128::from(*x)).sum()
}

fn verify_plain_amounts(inputs: &[BlindAssetRecord],
                        outputs: &[BlindAssetRecord])
                        -> Result<(), ZeiError> {
  let in_amount: Vec<u64> = inputs.iter()
                                  .map(|x| x.amount.get_amount().unwrap())
                                  .collect();
  let out_amount: Vec<u64> = outputs.iter()
                                    .map(|x| x.amount.get_amount().unwrap())
                                    .collect();

  let sum_inputs = safe_sum_u64(in_amount.as_slice());
  let sum_outputs = safe_sum_u64(out_amount.as_slice());

  if sum_inputs < sum_outputs {
    return Err(ZeiError::XfrVerifyAssetAmountError);
  }

  Ok(())
}

fn verify_plain_asset(inputs: &[BlindAssetRecord],
                      outputs: &[BlindAssetRecord])
                      -> Result<(), ZeiError> {
  let mut list = vec![];
  for x in inputs.iter() {
    list.push(x.asset_type.get_asset_type().unwrap());
  }
  for x in outputs.iter() {
    list.push(x.asset_type.get_asset_type().unwrap());
  }
  if list.iter().all_equal() {
    Ok(())
  } else {
    Err(ZeiError::XfrVerifyAssetAmountError)
  }
}

fn verify_plain_asset_mix(inputs: &[BlindAssetRecord],
                          outputs: &[BlindAssetRecord])
                          -> Result<(), ZeiError> {
  let mut amounts = HashMap::new();

  for record in inputs.iter() {
    match amounts.get_mut(&record.asset_type.get_asset_type().unwrap()) {
      None => {
        amounts.insert(record.asset_type.get_asset_type().unwrap(),
                       vec![i128::from(record.amount.get_amount().unwrap())]);
      }
      Some(vec) => {
        vec.push(i128::from(record.amount.get_amount().unwrap()));
      }
    };
  }

  for record in outputs.iter() {
    match amounts.get_mut(&record.asset_type.get_asset_type().unwrap()) {
      None => {
        amounts.insert(record.asset_type.get_asset_type().unwrap(),
                       vec![-i128::from(record.amount.get_amount().unwrap())]);
      }
      Some(vec) => {
        vec.push(-i128::from(record.amount.get_amount().unwrap()));
      }
    };
  }

  for (_, a) in amounts.iter() {
    let sum = a.iter().sum::<i128>();
    if sum < 0i128 {
      return Err(ZeiError::XfrVerifyAssetAmountError);
    }
  }
  Ok(())
}

fn batch_verify_asset_mix<R: CryptoRng + RngCore>(prng: &mut R,
                                                  params: &mut PublicParams,
                                                  bars_instances: &[(&[BlindAssetRecord],
                                                     &[BlindAssetRecord],
                                                     &AssetMixProof)])
                                                  -> Result<(), ZeiError> {
  fn process_bars(bars: &[BlindAssetRecord]) -> Vec<(CompressedRistretto, CompressedRistretto)> {
    let pow2_32 = Scalar::from(POW_2_32);
    bars.iter()
        .map(|x| {
          let (com_amount_low, com_amount_high) = match x.amount {
            XfrAmount::Confidential((c1, c2)) => {
              (c1.decompress().unwrap(), c2.decompress().unwrap())
            }
            XfrAmount::NonConfidential(amount) => {
              let pc_gens = PedersenGens::default();
              let (low, high) = u64_to_u32_pair(amount);
              (pc_gens.commit(Scalar::from(low), Scalar::zero()),
               pc_gens.commit(Scalar::from(high), Scalar::zero()))
            }
          };
          let com_amount = (com_amount_low + pow2_32 * com_amount_high).compress();

          let com_type = match x.asset_type {
            XfrAssetType::Confidential(c) => c,
            XfrAssetType::NonConfidential(asset_type) => {
              let scalar = asset_type_to_scalar(&asset_type);
              let pc_gens = PedersenGens::default();
              pc_gens.commit(scalar, Scalar::zero()).compress()
            }
          };
          (com_amount, com_type)
        })
        .collect_vec()
  }

  let mut asset_mix_instances = vec![];
  for instance in bars_instances {
    let in_coms = process_bars(instance.0);
    let out_coms = process_bars(instance.1);
    asset_mix_instances.push(AssetMixingInstance { inputs: in_coms,
                                                   outputs: out_coms,
                                                   proof: instance.2 });
  }
  batch_verify_asset_mixing(prng, params, &asset_mix_instances)
}

/*
fn verify_asset_mix<R: CryptoRng + RngCore>(prng: &mut R,
                                            params: &PublicParams,
                                            inputs: &[BlindAssetRecord],
                                            outputs: &[BlindAssetRecord],
                                            proof: &AssetMixProof)
                                            -> Result<(), ZeiError> {
  let pow2_32 = Scalar::from(POW_2_32);

  let mut in_coms = vec![];
  for x in inputs.iter() {
    let (com_amount_low, com_amount_high) = match x.amount {
      XfrAmount::Confidential((c1, c2)) => (c1.decompress().unwrap(), c2.decompress().unwrap()),
      XfrAmount::NonConfidential(amount) => {
        let pc_gens = PedersenGens::default();
        let (low, high) = u64_to_u32_pair(amount);
        (pc_gens.commit(Scalar::from(low), Scalar::zero()),
         pc_gens.commit(Scalar::from(high), Scalar::zero()))
      }
    };
    let com_amount = (com_amount_low + pow2_32 * com_amount_high).compress();

    let com_type = match x.asset_type {
      XfrAssetType::Confidential(c) => c,
      XfrAssetType::NonConfidential(asset_type) => {
        let scalar = asset_type_to_scalar(&asset_type);
        let pc_gens = PedersenGens::default();
        pc_gens.commit(scalar, Scalar::zero()).compress()
      }
    };
    in_coms.push((com_amount, com_type));
  }

  let mut out_coms = vec![];
  for x in outputs.iter() {
    // TODO avoid code duplication
    let (com_amount_low, com_amount_high) = match x.amount {
      XfrAmount::Confidential((c1, c2)) => (c1.decompress().unwrap(), c2.decompress().unwrap()),
      XfrAmount::NonConfidential(amount) => {
        let pc_gens = PedersenGens::default();
        let (low, high) = u64_to_u32_pair(amount);
        (pc_gens.commit(Scalar::from(low), Scalar::zero()),
         pc_gens.commit(Scalar::from(high), Scalar::zero()))
      }
    };
    let com_amount = (com_amount_low + pow2_32 * com_amount_high).compress();

    let com_type = match x.asset_type {
      XfrAssetType::Confidential(c) => c,
      XfrAssetType::NonConfidential(asset_type) => {
        let scalar = asset_type_to_scalar(&asset_type);
        let pc_gens = PedersenGens::default();
        pc_gens.commit(scalar, Scalar::zero()).compress()
      }
    };
    out_coms.push((com_amount, com_type));
  }
  let instance = AssetMixingIntance { inputs: &in_coms,
                                      outputs: &out_coms,
                                      proof };
  batch_verify_asset_mixing(prng, params, &[instance])
}
*/

// ASSET TRACKING
pub fn find_tracing_memos<'a>(
  xfr_body: &'a XfrBody,
  pub_key: &AssetTracerEncKeys)
  -> Result<Vec<(&'a BlindAssetRecord, &'a AssetTracerMemo)>, ZeiError> {
  let mut result = vec![];
  if xfr_body.inputs.len() + xfr_body.outputs.len() != xfr_body.asset_tracing_memos.len() {
    return Err(ZeiError::InconsistentStructureError);
  }
  for (blind_asset_record, bar_memos) in xfr_body.inputs
                                                 .iter()
                                                 .chain(&xfr_body.outputs)
                                                 .zip(&xfr_body.asset_tracing_memos)
  {
    for memo in bar_memos {
      if memo.enc_key == *pub_key {
        result.push((blind_asset_record, memo));
      }
    }
  }
  Ok(result)
}

/// amount, asset type, identity attribute, public key
pub type RecordData = (u64, AssetType, Vec<u32>, XfrPublicKey);

pub fn extract_tracking_info(memos: &[(&BlindAssetRecord, &AssetTracerMemo)],
                             dec_key: &AssetTracerDecKeys,
                             candidate_asset_types: &[AssetType])
                             -> Result<Vec<RecordData>, ZeiError> {
  let mut result = vec![];
  for bar_memo in memos {
    let blind_asset_record = bar_memo.0;
    let memo = bar_memo.1;
    let amount = match memo.lock_amount {
      None => blind_asset_record.amount
                                .get_amount()
                                .ok_or(ZeiError::InconsistentStructureError)?,
      Some(_) => memo.extract_amount_brute_force(&dec_key.record_data_dec_key)?,
    };

    let asset_type = match memo.lock_asset_type {
      None => blind_asset_record.asset_type
                                .get_asset_type()
                                .ok_or(ZeiError::InconsistentStructureError)?,
      Some(_) => memo.extract_asset_type(&dec_key.record_data_dec_key, candidate_asset_types)?,
    };

    let attributes = match memo.lock_attributes {
      None => vec![],
      _ => memo.extract_identity_attributes_brute_force(&dec_key.attrs_dec_key)?,
    };
    result.push((amount, asset_type, attributes, blind_asset_record.public_key));
  }
  Ok(result)
}

pub fn trace_assets(xfr_body: &XfrBody,
                    tracer_keypair: &AssetTracerKeyPair,
                    candidate_assets: &[AssetType])
                    -> Result<Vec<RecordData>, ZeiError> {
  let bars_memos = find_tracing_memos(xfr_body, &tracer_keypair.enc_key)?;
  extract_tracking_info(bars_memos.as_slice(),
                        &tracer_keypair.dec_key,
                        candidate_assets)
}

pub fn verify_tracing_memos(memos: &[(&BlindAssetRecord, &AssetTracerMemo)],
                            dec_key: &AssetTracerDecKeys,
                            expected_data: &[RecordData])
                            -> Result<(), ZeiError> {
  if memos.len() != expected_data.len() {
    return Err(ZeiError::ParameterError);
  }
  for (bar_memo, expected) in memos.iter().zip(expected_data) {
    let blind_asset_record = bar_memo.0;
    let memo = bar_memo.1;
    match memo.lock_amount {
      None => {
        let bar_amount = blind_asset_record.amount
                                           .get_amount()
                                           .ok_or(ZeiError::InconsistentStructureError)?;
        if bar_amount != expected.0 {
          return Err(ZeiError::AssetTracingExtractionError);
        }
      }
      Some(_) => memo.verify_amount(&dec_key.record_data_dec_key, expected.0)?,
    };

    match memo.lock_asset_type {
      None => {
        let asset_type = blind_asset_record.asset_type
                                           .get_asset_type()
                                           .ok_or(ZeiError::InconsistentStructureError)?;
        if asset_type != expected.1 {
          return Err(ZeiError::AssetTracingExtractionError);
        }
      }
      Some(_) => {
        memo.extract_asset_type(&dec_key.record_data_dec_key, &[expected.1])?;
      }
    };
    if memo.lock_attributes.is_some() {
      let result =
        memo.verify_identity_attributes(&dec_key.attrs_dec_key, (expected.2).as_slice())?;
      if !result.iter().all(|current| *current) {
        return Err(ZeiError::IdentityTracingExtractionError);
      }
    };
  }
  Ok(())
}

pub fn verify_tracing_ctexts(xfr_body: &XfrBody,
                             tracer_keypair: &AssetTracerKeyPair,
                             expected_data: &[RecordData])
                             -> Result<(), ZeiError> {
  let bars_memos = find_tracing_memos(xfr_body, &tracer_keypair.enc_key)?;
  verify_tracing_memos(bars_memos.as_slice(),
                       &tracer_keypair.dec_key,
                       expected_data)
}
