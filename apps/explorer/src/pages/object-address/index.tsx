// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { useParams } from 'react-router-dom';
import {
	isSuiNSName,
	useGetObject,
	useResolveSuiNSAddress,
	useResolveSuiNSName,
} from '@mysten/core';
import { PageLayout } from '~/components/Layout/PageLayout';
import { PageHeader } from '~/ui/PageHeader';
import { ObjectDetailsHeader } from '@mysten/icons';
import { TotalStaked } from '~/pages/address-result/TotalStaked';
import { ErrorBoundary } from '~/components/error-boundary/ErrorBoundary';
import { ObjectView } from '~/pages/object-result/views/ObjectView';
import { OwnedCoins } from '~/components/OwnedCoins';
import { OwnedObjects } from '~/components/OwnedObjects';
import { LOCAL_STORAGE_SPLIT_PANE_KEYS, SplitPanes } from '~/ui/SplitPanes';
import { TabHeader } from '~/ui/Tabs';
import { Banner } from '~/ui/Banner';
import { FieldsContent } from '~/pages/object-result/views/TokenView';
import { Divider } from '~/ui/Divider';
import { Heading } from '@mysten/ui';
import { useBreakpoint } from '~/hooks/useBreakpoint';
import TransactionBlocksForAddress from '~/components/TransactionBlocksForAddress';
import { TransactionsForAddress } from '~/components/transactions/TransactionsForAddress';

const LEFT_RIGHT_PANEL_MIN_SIZE = 30;

function Header({
	address,
	loading,
	error,
}: {
	address: string;
	loading?: boolean;
	error?: Error | null;
}) {
	const { data: domainName, isLoading, error: resolveSuinsError } = useResolveSuiNSName(address);
	const { data, isPending, error: getObjectError } = useGetObject(address!);
	const errorText = getObjectError?.message ?? resolveSuinsError?.message ?? error?.message;

	return (
		<div>
			<PageHeader
				error={errorText}
				loading={loading || isLoading || isPending}
				type="Address"
				title={address}
				subtitle={domainName}
				before={<ObjectDetailsHeader className="h-6 w-6" />}
				after={<TotalStaked address={address} />}
			/>

			<ErrorBoundary>
				{data && (
					<div className="mt-5">
						<ObjectView data={data} />
					</div>
				)}
			</ErrorBoundary>
		</div>
	);
}

function OwnedObjectsSection({ address }: { address: string }) {
	const isMediumOrAbove = useBreakpoint('md');

	const leftPane = {
		panel: (
			<div className="mb-5">
				<OwnedCoins id={address} />
			</div>
		),
		minSize: LEFT_RIGHT_PANEL_MIN_SIZE,
		defaultSize: LEFT_RIGHT_PANEL_MIN_SIZE,
	};

	const rightPane = {
		panel: <OwnedObjects id={address} />,
		minSize: LEFT_RIGHT_PANEL_MIN_SIZE,
	};

	return (
		<TabHeader title="Owned Objects" noGap>
			<div className="flex h-full flex-col justify-between">
				<ErrorBoundary>
					{isMediumOrAbove ? (
						<SplitPanes
							autoSaveId={LOCAL_STORAGE_SPLIT_PANE_KEYS.ADDRESS_VIEW_HORIZONTAL}
							dividerSize="none"
							splitPanels={[leftPane, rightPane]}
							direction="horizontal"
						/>
					) : (
						<>
							{leftPane.panel}
							<div className="my-8">
								<Divider />
							</div>
							{rightPane.panel}
						</>
					)}
				</ErrorBoundary>
			</div>
		</TabHeader>
	);
}

function ObjectAddressContent({ address, error }: { address: string; error?: Error | null }) {
	if (error) {
		return (
			<Banner variant="error" spacing="lg" fullWidth>
				Data could not be extracted on the following specified address ID: {address}
			</Banner>
		);
	}

	return (
		<div>
			<section>
				<OwnedObjectsSection address={address} />
			</section>

			<Divider />

			<section className="mt-14">
				<FieldsContent objectId={address} />
			</section>

			<section className="mt-14 flex flex-col md:flex-row">
				<div className="basis-1/2 pr-0 md:pr-5">
					<div className="border-b border-gray-45 pb-5">
						<Heading color="gray-90" variant="heading4/semibold">
							Address Transaction Blocks
						</Heading>
					</div>

					<div className="pt-5">
						<TransactionsForAddress address={address} type="address" />
					</div>
				</div>

				<Divider vertical />

				<div className="mt-14 basis-1/2 pl-0 md:mt-0 md:pl-5">
					<TransactionBlocksForAddress
						noBorderBottom
						address={address}
						isObject
						tableHeader="Object Transaction Blocks"
					/>
				</div>
			</section>
		</div>
	);
}

function SuinsPageLayoutContainer({ name }: { name: string }) {
	const { data: address, isLoading, error } = useResolveSuiNSAddress(name);

	return <PageLayoutContainer address={address ?? name} loading={isLoading} error={error} />;
}

function PageLayoutContainer({
	address,
	loading,
	error,
}: {
	address: string;
	loading?: boolean;
	error?: Error | null;
}) {
	return (
		<PageLayout
			loading={loading}
			isError={!!error}
			gradient={{
				size: 'md',
				content: <Header address={address} />,
			}}
			content={<ObjectAddressContent address={address} error={error} />}
		/>
	);
}

export function ObjectAddress() {
	const { id } = useParams();
	const isSuiNSAddress = isSuiNSName(id!);

	if (isSuiNSAddress) {
		return <SuinsPageLayoutContainer name={id!} />;
	}

	return <PageLayoutContainer address={id!} />;
}
